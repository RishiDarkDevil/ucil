/**
 * @fileoverview Top-level task manager facade.
 *
 * {@link TaskManager} is the primary public API of the typescript-project
 * fixture library.  It orchestrates the {@link TaskRepository} (storage) and
 * {@link FilterEngine} (query) layers and emits lifecycle events to registered
 * listeners.
 *
 * All mutating operations are wrapped in a lightweight snapshot-based
 * transaction that rolls back when an error is thrown.
 */

import {
  type Task,
  type CreateTaskInput,
  type UpdateTaskInput,
  type Filter,
  type Query,
  type QueryResult,
  type AggregateResult,
  type TaskEventType,
  type TaskEvent,
  type TaskStats,
  TASK_STATUS,
  TASK_PRIORITY,
  generateId,
  validateTitle,
  validateTaskStatus,
  validatePriority,
  validateTags,
  validateMetadata,
  validateHours,
  cloneTask,
  serializeTask,
  parseTaskFromJSON,
  isOverdue,
  isValidStatusTransition,
  comparePriority,
  TaskNotFoundError,
  ValidationError,
  QueryError,
  DEFAULT_QUERY_LIMIT,
} from "./types.js";
import { TaskRepository } from "./repository.js";
import { FilterEngine } from "./filter-engine.js";

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/**
 * Returns `now` as a `Date`.  Extracted so tests can inject a fixed time via
 * the optional constructor argument.
 */
type ClockFn = () => Date;

/** Default wall-clock function. */
const WALL_CLOCK: ClockFn = () => new Date();

/**
 * Builds a fully-formed {@link Task} from {@link CreateTaskInput} and a
 * generated id.
 */
function buildTask(input: CreateTaskInput, id: string, now: Date): Task {
  const title = validateTitle(input.title);
  const status = input.status !== undefined ? validateTaskStatus(input.status) : "pending";
  const priority = input.priority !== undefined ? validatePriority(input.priority) : "medium";
  const tags = input.tags !== undefined ? validateTags(input.tags) : [];
  const metadata = input.metadata !== undefined ? validateMetadata(input.metadata) : {};
  const estimatedHours = validateHours(input.estimatedHours ?? null, "estimatedHours");
  const actualHours = validateHours(input.actualHours ?? null, "actualHours");

  return {
    id,
    title,
    description: input.description ?? "",
    status,
    priority,
    tags,
    assignee: input.assignee ?? null,
    dueDate: input.dueDate ?? null,
    createdAt: now,
    updatedAt: now,
    parentId: input.parentId ?? null,
    subtaskIds: [],
    metadata,
    estimatedHours,
    actualHours,
  };
}

/**
 * Merges an {@link UpdateTaskInput} onto an existing {@link Task} and returns
 * the patched copy.  Does not mutate the source task.
 */
function applyUpdate(task: Task, input: UpdateTaskInput, now: Date): Task {
  const patch: Partial<Task> = { updatedAt: now };

  if (input.title !== undefined) patch.title = validateTitle(input.title);
  if (input.description !== undefined) patch.description = input.description;
  if (input.status !== undefined) patch.status = validateTaskStatus(input.status);
  if (input.priority !== undefined) patch.priority = validatePriority(input.priority);
  if (input.tags !== undefined) patch.tags = validateTags(input.tags);
  if ("assignee" in input) patch.assignee = input.assignee ?? null;
  if ("dueDate" in input) patch.dueDate = input.dueDate ?? null;
  if ("parentId" in input) patch.parentId = input.parentId ?? null;
  if (input.metadata !== undefined) patch.metadata = validateMetadata(input.metadata);
  if ("estimatedHours" in input) {
    patch.estimatedHours = validateHours(input.estimatedHours ?? null, "estimatedHours");
  }
  if ("actualHours" in input) {
    patch.actualHours = validateHours(input.actualHours ?? null, "actualHours");
  }

  return { ...task, ...patch };
}

// ---------------------------------------------------------------------------
// TaskManager
// ---------------------------------------------------------------------------

/**
 * The primary API for creating, querying, and managing tasks.
 *
 * @example
 * ```typescript
 * const manager = new TaskManager();
 * const task = manager.createTask({ title: "Fix login bug", priority: "high" });
 * const pending = manager.findAll({ field: "status", operator: "eq", value: "pending" });
 * console.log(pending.length); // 1
 * ```
 */
export class TaskManager {
  /** Underlying storage layer. */
  private readonly repository: TaskRepository;

  /** Query / sort evaluation engine. */
  private readonly filterEngine: FilterEngine;

  /** Event listener registry, keyed by event type. */
  private readonly eventListeners: Map<
    TaskEventType,
    Array<(event: TaskEvent) => void>
  >;

  /** Pluggable clock; defaults to wall clock for testability. */
  private readonly clock: ClockFn;

  /**
   * Creates a new empty task manager.
   *
   * @param clock — Optional override for the current-time function.  Useful
   *                in tests to produce deterministic timestamps.
   */
  constructor(clock: ClockFn = WALL_CLOCK) {
    this.repository = new TaskRepository();
    this.filterEngine = new FilterEngine();
    this.eventListeners = new Map();
    this.clock = clock;
  }

  // -------------------------------------------------------------------------
  // CRUD: single-task operations
  // -------------------------------------------------------------------------

  /**
   * Creates a new task from `input`, stores it in the repository, and emits a
   * `"created"` event.
   *
   * @param input — Required/optional fields for the new task.
   * @returns The newly created task.
   * @throws {@link ValidationError} for invalid field values.
   */
  createTask(input: CreateTaskInput): Task {
    const id = generateId();
    const now = this.clock();
    const task = buildTask(input, id, now);

    this.transaction(() => {
      this.repository.insert(task);
      // If it has a parent, add this task's id to the parent's subtaskIds
      if (task.parentId !== null) {
        this.linkChildToParent(task.id, task.parentId);
      }
    });

    this.emit({
      type: "created",
      taskId: task.id,
      timestamp: now,
      newValue: cloneTask(task),
    });

    const stored = this.repository.findById(task.id);
    if (stored === undefined) throw new Error("Invariant violation: task missing after insert.");
    return stored;
  }

  /**
   * Updates the task identified by `id` with the fields in `input`.
   *
   * Emits appropriate lifecycle events when `status`, `priority`, or
   * `assignee` changes.
   *
   * @param id    — Task id to update.
   * @param input — Fields to change.
   * @returns The updated task.
   * @throws {@link TaskNotFoundError} when `id` does not exist.
   * @throws {@link ValidationError} for invalid field values.
   */
  updateTask(id: string, input: UpdateTaskInput): Task {
    const existing = this.repository.findById(id);
    if (existing === undefined) throw new TaskNotFoundError(id);

    const now = this.clock();
    const updated = applyUpdate(existing, input, now);

    this.transaction(() => {
      // Handle parent change: remove from old parent, add to new parent
      if (updated.parentId !== existing.parentId) {
        if (existing.parentId !== null) {
          this.unlinkChildFromParent(id, existing.parentId);
        }
        if (updated.parentId !== null) {
          this.linkChildToParent(id, updated.parentId);
        }
      }
      this.repository.update(id, updated);
    });

    // Emit specific events for notable field changes
    if (updated.status !== existing.status) {
      this.emit({
        type: "statusChanged",
        taskId: id,
        timestamp: now,
        previousValue: existing.status,
        newValue: updated.status,
      });
    }
    if (updated.priority !== existing.priority) {
      this.emit({
        type: "priorityChanged",
        taskId: id,
        timestamp: now,
        previousValue: existing.priority,
        newValue: updated.priority,
      });
    }
    if (updated.assignee !== existing.assignee) {
      this.emit({
        type: "assigned",
        taskId: id,
        timestamp: now,
        previousValue: existing.assignee,
        newValue: updated.assignee,
      });
    }
    // Always emit a generic "updated"
    this.emit({
      type: "updated",
      taskId: id,
      timestamp: now,
      previousValue: cloneTask(existing),
      newValue: cloneTask(updated),
    });

    const stored = this.repository.findById(id);
    if (stored === undefined) throw new Error("Invariant violation: task missing after update.");
    return stored;
  }

  /**
   * Deletes the task identified by `id` from the repository.
   *
   * Also removes the id from its parent's `subtaskIds` list (if applicable)
   * and recursively deletes all descendants.
   *
   * @param id — Task id to delete.
   * @throws {@link TaskNotFoundError} when `id` does not exist.
   */
  deleteTask(id: string): void {
    const existing = this.repository.findById(id);
    if (existing === undefined) throw new TaskNotFoundError(id);
    const now = this.clock();

    this.transaction(() => {
      this.deleteTaskRecursive(id);
    });

    this.emit({
      type: "deleted",
      taskId: id,
      timestamp: now,
      previousValue: cloneTask(existing),
    });
  }

  /**
   * Returns the task with the given `id`.
   *
   * @param id — Task id to retrieve.
   * @throws {@link TaskNotFoundError} when `id` does not exist.
   */
  getTask(id: string): Task {
    const task = this.repository.findById(id);
    if (task === undefined) throw new TaskNotFoundError(id);
    return task;
  }

  // -------------------------------------------------------------------------
  // CRUD: bulk operations
  // -------------------------------------------------------------------------

  /**
   * Creates multiple tasks atomically.
   *
   * If any input fails validation the whole batch is rolled back.
   *
   * @param inputs — Array of task creation inputs.
   * @returns Array of created tasks in the same order as `inputs`.
   */
  createMany(inputs: CreateTaskInput[]): Task[] {
    const now = this.clock();
    const tasks: Task[] = inputs.map((inp) => buildTask(inp, generateId(), now));

    this.transaction(() => {
      for (const task of tasks) {
        this.repository.insert(task);
        if (task.parentId !== null) {
          this.linkChildToParent(task.id, task.parentId);
        }
      }
    });

    for (const task of tasks) {
      this.emit({ type: "created", taskId: task.id, timestamp: now, newValue: cloneTask(task) });
    }

    return tasks.map((t) => {
      const stored = this.repository.findById(t.id);
      if (stored === undefined) throw new Error("Invariant violation after bulk insert.");
      return stored;
    });
  }

  /**
   * Updates all tasks that match `filter`, applying the same `patch` to each.
   *
   * @param filter — Filter that selects the tasks to update.
   * @param patch  — Fields to overwrite on each matched task.
   * @returns Array of updated tasks.
   */
  updateMany(filter: Filter, patch: UpdateTaskInput): Task[] {
    const matched = this.findAll(filter);
    const results: Task[] = [];
    for (const task of matched) {
      results.push(this.updateTask(task.id, patch));
    }
    return results;
  }

  /**
   * Deletes all tasks that match `filter`.
   *
   * @param filter — Filter that selects the tasks to delete.
   * @returns The number of tasks deleted.
   */
  deleteMany(filter: Filter): number {
    const matched = this.findAll(filter);
    for (const task of matched) {
      if (this.repository.has(task.id)) {
        this.deleteTask(task.id);
      }
    }
    return matched.length;
  }

  // -------------------------------------------------------------------------
  // Queries
  // -------------------------------------------------------------------------

  /**
   * Executes a full {@link Query} (filter + sort + pagination + projection)
   * against all stored tasks.
   *
   * @param q — The query descriptor.
   * @returns A paginated result set.
   * @throws {@link QueryError} for malformed filters or sort specifications.
   */
  query(q: Query): QueryResult {
    const all = this.repository.findAll();
    return this.filterEngine.applyQuery(all, q);
  }

  /**
   * Returns all tasks that satisfy `filter`, or all tasks when no filter is
   * given.
   *
   * @param filter — Optional filter tree.
   */
  findAll(filter?: Filter): Task[] {
    const all = this.repository.findAll();
    if (filter === undefined) return all;
    return this.filterEngine.applyFilter(all, filter);
  }

  /**
   * Returns the number of tasks that satisfy `filter`.
   *
   * @param filter — Optional filter tree.
   */
  count(filter?: Filter): number {
    return this.findAll(filter).length;
  }

  // -------------------------------------------------------------------------
  // Aggregation
  // -------------------------------------------------------------------------

  /**
   * Groups tasks by the given `groupBy` field and returns an
   * {@link AggregateResult} with per-group counts.
   *
   * @param groupBy — A field whose value is used as the grouping key.
   * @param filter  — Optional pre-filter.
   */
  aggregate(groupBy: keyof Task, filter?: Filter): AggregateResult {
    const tasks = this.findAll(filter);
    const groups = new Map<string, Task[]>();

    for (const task of tasks) {
      const key = String(task[groupBy] ?? "(null)");
      let bucket = groups.get(key);
      if (bucket === undefined) {
        bucket = [];
        groups.set(key, bucket);
      }
      bucket.push(task);
    }

    const groupByResult: Record<string, AggregateResult> = {};
    for (const [key, bucket] of groups) {
      groupByResult[key] = {
        count: bucket.length,
        sum: {
          estimatedHours: sumField(bucket, "estimatedHours"),
          actualHours: sumField(bucket, "actualHours"),
        },
        avg: {
          estimatedHours: avgField(bucket, "estimatedHours"),
          actualHours: avgField(bucket, "actualHours"),
        },
      };
    }

    return {
      count: tasks.length,
      sum: {
        estimatedHours: sumField(tasks, "estimatedHours"),
        actualHours: sumField(tasks, "actualHours"),
      },
      avg: {
        estimatedHours: avgField(tasks, "estimatedHours"),
        actualHours: avgField(tasks, "actualHours"),
      },
      groupBy: groupByResult,
    };
  }

  /**
   * Returns the sum of `field` across all tasks that match `filter`.
   *
   * Null values are excluded from the sum (treated as 0).
   *
   * @param field  — `"estimatedHours"` or `"actualHours"`.
   * @param filter — Optional pre-filter.
   */
  sumField(
    field: "estimatedHours" | "actualHours",
    filter?: Filter
  ): number {
    const tasks = this.findAll(filter);
    return sumField(tasks, field);
  }

  // -------------------------------------------------------------------------
  // Hierarchy
  // -------------------------------------------------------------------------

  /**
   * Returns the direct subtasks of `parentId`.
   *
   * @param parentId — Id of the parent task.
   * @throws {@link TaskNotFoundError} when `parentId` does not exist.
   */
  getSubtasks(parentId: string): Task[] {
    if (!this.repository.has(parentId)) throw new TaskNotFoundError(parentId);
    return this.repository.findByParent(parentId);
  }

  /**
   * Returns the chain of ancestor tasks from direct parent up to the root,
   * in bottom-up order (first element = direct parent, last = root).
   *
   * @param taskId — Id of the task whose ancestors to retrieve.
   * @throws {@link TaskNotFoundError} when `taskId` does not exist.
   */
  getAncestors(taskId: string): Task[] {
    const task = this.repository.findById(taskId);
    if (task === undefined) throw new TaskNotFoundError(taskId);

    const ancestors: Task[] = [];
    const visited = new Set<string>();
    let current = task.parentId;

    while (current !== null) {
      if (visited.has(current)) {
        throw new QueryError(taskId, `Cycle detected in task hierarchy at "${current}".`);
      }
      visited.add(current);
      const parent = this.repository.findById(current);
      if (parent === undefined) break;
      ancestors.push(parent);
      current = parent.parentId;
    }

    return ancestors;
  }

  /**
   * Moves `taskId` to a new parent, updating both the old and new parent's
   * `subtaskIds` lists as well as `task.parentId`.
   *
   * Pass `newParentId = null` to promote the task to a root task.
   *
   * @param taskId      — The task to move.
   * @param newParentId — The new parent id, or `null` for root.
   * @returns The updated task.
   * @throws {@link TaskNotFoundError} when either id does not exist.
   * @throws {@link QueryError} when the move would create a cycle.
   */
  moveTask(taskId: string, newParentId: string | null): Task {
    if (!this.repository.has(taskId)) throw new TaskNotFoundError(taskId);
    if (newParentId !== null && !this.repository.has(newParentId)) {
      throw new TaskNotFoundError(newParentId);
    }

    // Cycle check: newParentId must not be a descendant of taskId
    if (newParentId !== null) {
      const descendants = this.collectDescendantIds(taskId);
      if (descendants.has(newParentId)) {
        throw new QueryError(
          taskId,
          `Cannot move task "${taskId}" under "${newParentId}": would create a cycle.`
        );
      }
    }

    return this.updateTask(taskId, { parentId: newParentId });
  }

  // -------------------------------------------------------------------------
  // Events
  // -------------------------------------------------------------------------

  /**
   * Registers `listener` to be called whenever an event of `type` is emitted.
   *
   * @param type     — The event type to subscribe to.
   * @param listener — The callback function.
   */
  on(type: TaskEventType, listener: (e: TaskEvent) => void): void {
    let bucket = this.eventListeners.get(type);
    if (bucket === undefined) {
      bucket = [];
      this.eventListeners.set(type, bucket);
    }
    bucket.push(listener);
  }

  /**
   * Unregisters a previously registered listener.
   *
   * If `listener` was not registered, this is a no-op.
   *
   * @param type     — The event type.
   * @param listener — The callback to remove.
   */
  off(type: TaskEventType, listener: (e: TaskEvent) => void): void {
    const bucket = this.eventListeners.get(type);
    if (bucket === undefined) return;
    const idx = bucket.indexOf(listener);
    if (idx !== -1) {
      bucket.splice(idx, 1);
    }
  }

  // -------------------------------------------------------------------------
  // Transactions
  // -------------------------------------------------------------------------

  /**
   * Executes `fn` inside a repository transaction.
   *
   * If `fn` throws, the repository is restored to the pre-call snapshot and
   * the error is re-thrown.  If `fn` succeeds, its return value is returned.
   *
   * @param fn — The operation to execute transactionally.
   * @returns The return value of `fn`.
   */
  transaction<T>(fn: () => T): T {
    const snap = this.repository.snapshot();
    try {
      return fn();
    } catch (err) {
      this.repository.restore(snap);
      throw err;
    }
  }

  // -------------------------------------------------------------------------
  // Statistics
  // -------------------------------------------------------------------------

  /**
   * Returns a summary of the current task set.
   *
   * @param now — Optional reference time for overdue calculation.
   */
  getStats(now?: Date): TaskStats {
    const ref = now ?? this.clock();
    const all = this.repository.findAll();
    const byStatus = {} as Record<(typeof TASK_STATUS)[number], number>;
    for (const s of TASK_STATUS) byStatus[s] = 0;
    const byPriority = {} as Record<(typeof TASK_PRIORITY)[number], number>;
    for (const p of TASK_PRIORITY) byPriority[p] = 0;
    let overdue = 0;
    for (const task of all) {
      byStatus[task.status]++;
      byPriority[task.priority]++;
      if (isOverdue(task, ref)) overdue++;
    }
    return { total: all.length, byStatus, byPriority, overdue };
  }

  // -------------------------------------------------------------------------
  // Import / Export
  // -------------------------------------------------------------------------

  /**
   * Serialises all tasks to a JSON string.
   *
   * The output is a JSON array of {@link SerializedTask} objects.  Use
   * {@link importFromJSON} to restore.
   *
   * @returns A JSON string.
   */
  exportToJSON(): string {
    const tasks = this.repository.findAll();
    return JSON.stringify(tasks.map(serializeTask), null, 2);
  }

  /**
   * Imports tasks from a JSON string previously produced by {@link exportToJSON}.
   *
   * Existing tasks are NOT cleared.  Tasks whose id already exists are skipped
   * (not overwritten) to preserve local mutations.
   *
   * @param json — JSON string of a `SerializedTask[]`.
   * @returns The number of tasks actually imported (skipping duplicates).
   * @throws {@link ValidationError} when the JSON is malformed.
   */
  importFromJSON(json: string): number {
    let raw: unknown;
    try {
      raw = JSON.parse(json) as unknown;
    } catch (e) {
      throw new ValidationError("(json)", json, `Failed to parse JSON: ${String(e)}`);
    }
    if (!Array.isArray(raw)) {
      throw new ValidationError("(json)", raw, "Expected a JSON array.");
    }
    let imported = 0;
    for (const item of raw) {
      const task = parseTaskFromJSON(item);
      if (!this.repository.has(task.id)) {
        this.repository.insert(task);
        imported++;
      }
    }
    return imported;
  }

  // -------------------------------------------------------------------------
  // Status transition validation
  // -------------------------------------------------------------------------

  /**
   * Validates and applies a status transition for `taskId`.
   *
   * Enforces the state machine defined in {@link isValidStatusTransition}.
   *
   * @param taskId    — Id of the task to transition.
   * @param newStatus — The desired new status.
   * @returns The updated task.
   * @throws {@link ValidationError} when the transition is not allowed.
   * @throws {@link TaskNotFoundError} when `taskId` does not exist.
   */
  transitionStatus(
    taskId: string,
    newStatus: (typeof TASK_STATUS)[number]
  ): Task {
    const task = this.getTask(taskId);
    if (!isValidStatusTransition(task.status, newStatus)) {
      throw new ValidationError(
        "status",
        newStatus,
        `Transition from "${task.status}" to "${newStatus}" is not allowed.`
      );
    }
    return this.updateTask(taskId, { status: newStatus });
  }

  // -------------------------------------------------------------------------
  // Convenience query methods
  // -------------------------------------------------------------------------

  /**
   * Returns all tasks with `status = "pending"` sorted by priority descending.
   */
  getPendingTasks(): Task[] {
    const tasks = this.findAll({ field: "status", operator: "eq", value: "pending" });
    return [...tasks].sort((a, b) => comparePriority(b.priority, a.priority));
  }

  /**
   * Returns all overdue tasks (past due date, non-terminal status).
   *
   * @param now — Optional reference time; defaults to clock.
   */
  getOverdueTasks(now?: Date): Task[] {
    const ref = now ?? this.clock();
    return this.repository.findAll().filter((t) => isOverdue(t, ref));
  }

  /**
   * Returns tasks assigned to `assignee`.
   *
   * @param assignee — Username / user-id.
   */
  getTasksByAssignee(assignee: string): Task[] {
    return this.repository.findByAssignee(assignee);
  }

  /**
   * Returns tasks tagged with `tag`.
   *
   * @param tag — The tag string.
   */
  getTasksByTag(tag: string): Task[] {
    return this.repository.findByTag(tag);
  }

  /**
   * Searches tasks by a substring match on `title` or `description`.
   *
   * Case-insensitive.
   *
   * @param text — The substring to search for.
   */
  search(text: string): Task[] {
    const lower = text.toLowerCase();
    return this.repository.findAll().filter(
      (t) =>
        t.title.toLowerCase().includes(lower) ||
        t.description.toLowerCase().includes(lower)
    );
  }

  /**
   * Returns all root tasks (tasks with `parentId = null`).
   */
  getRootTasks(): Task[] {
    return this.repository.findAll().filter((t) => t.parentId === null);
  }

  /**
   * Returns the full subtree of tasks rooted at `rootId` (depth-first).
   *
   * @param rootId — Id of the root of the subtree.
   * @throws {@link TaskNotFoundError} when `rootId` does not exist.
   */
  getSubtree(rootId: string): Task[] {
    const root = this.getTask(rootId);
    const result: Task[] = [root];
    const queue: string[] = [...root.subtaskIds];
    const visited = new Set<string>([rootId]);
    while (queue.length > 0) {
      const id = queue.shift()!;
      if (visited.has(id)) continue;
      visited.add(id);
      const task = this.repository.findById(id);
      if (task !== undefined) {
        result.push(task);
        queue.push(...task.subtaskIds);
      }
    }
    return result;
  }

  /**
   * Returns the depth of `taskId` in the hierarchy (root tasks have depth 0).
   *
   * @param taskId — The task id.
   * @throws {@link TaskNotFoundError} when `taskId` does not exist.
   */
  getDepth(taskId: string): number {
    return this.getAncestors(taskId).length;
  }

  /**
   * Adds `tag` to the task's tags if it is not already present.
   *
   * @param taskId — Id of the task.
   * @param tag    — The tag to add.
   * @returns The updated task.
   */
  addTag(taskId: string, tag: string): Task {
    const task = this.getTask(taskId);
    if (task.tags.includes(tag)) return task;
    return this.updateTask(taskId, { tags: [...task.tags, tag] });
  }

  /**
   * Removes `tag` from the task's tags.
   *
   * @param taskId — Id of the task.
   * @param tag    — The tag to remove.
   * @returns The updated task.
   */
  removeTag(taskId: string, tag: string): Task {
    const task = this.getTask(taskId);
    return this.updateTask(taskId, {
      tags: task.tags.filter((t) => t !== tag),
    });
  }

  /**
   * Sets a single metadata key on the task.
   *
   * @param taskId — Id of the task.
   * @param key    — Metadata key.
   * @param value  — Metadata value.
   * @returns The updated task.
   */
  setMetadata(
    taskId: string,
    key: string,
    value: string | number | boolean
  ): Task {
    const task = this.getTask(taskId);
    return this.updateTask(taskId, {
      metadata: { ...task.metadata, [key]: value },
    });
  }

  /**
   * Removes a single metadata key from the task.
   *
   * @param taskId — Id of the task.
   * @param key    — Metadata key to remove.
   * @returns The updated task.
   */
  deleteMetadata(taskId: string, key: string): Task {
    const task = this.getTask(taskId);
    const metadata = { ...task.metadata };
    delete metadata[key];
    return this.updateTask(taskId, { metadata });
  }

  // -------------------------------------------------------------------------
  // Pagination helper
  // -------------------------------------------------------------------------

  /**
   * Returns a page of tasks matching an optional filter.
   *
   * @param page     — 1-based page number.
   * @param pageSize — Items per page; defaults to {@link DEFAULT_QUERY_LIMIT}.
   * @param filter   — Optional filter.
   * @param sort     — Optional sort specification.
   */
  paginate(
    page: number,
    pageSize: number = DEFAULT_QUERY_LIMIT,
    filter?: Filter,
    sort?: import("./types.js").SortSpec[]
  ): QueryResult {
    if (page < 1) throw new ValidationError("page", page, "Page number must be ≥ 1.");
    if (pageSize < 1) throw new ValidationError("pageSize", pageSize, "Page size must be ≥ 1.");
    return this.query({
      filter,
      sort,
      offset: (page - 1) * pageSize,
      limit: pageSize,
    });
  }

  // -------------------------------------------------------------------------
  // Private helpers
  // -------------------------------------------------------------------------

  /**
   * Emits `event` to all registered listeners for `event.type`.
   *
   * Listener errors are caught and logged to avoid crashing the manager.
   */
  private emit(event: TaskEvent): void {
    const bucket = this.eventListeners.get(event.type);
    if (bucket === undefined) return;
    for (const listener of [...bucket]) {
      try {
        listener(event);
      } catch (err) {
        // Listeners must not crash the manager.
        console.error(`TaskManager: event listener for "${event.type}" threw:`, err);
      }
    }
  }

  /**
   * Adds `childId` to the `subtaskIds` array of task `parentId`.
   *
   * Called during insert / move operations inside a transaction.
   */
  private linkChildToParent(childId: string, parentId: string): void {
    const parent = this.repository.findById(parentId);
    if (parent === undefined) throw new TaskNotFoundError(parentId);
    if (!parent.subtaskIds.includes(childId)) {
      this.repository.update(parentId, {
        subtaskIds: [...parent.subtaskIds, childId],
        updatedAt: this.clock(),
      });
    }
  }

  /**
   * Removes `childId` from the `subtaskIds` array of task `parentId`.
   */
  private unlinkChildFromParent(childId: string, parentId: string): void {
    const parent = this.repository.findById(parentId);
    if (parent === undefined) return; // parent already deleted is fine
    this.repository.update(parentId, {
      subtaskIds: parent.subtaskIds.filter((id) => id !== childId),
      updatedAt: this.clock(),
    });
  }

  /**
   * Deletes `id` and all descendants recursively.
   *
   * Also unlinks the task from its parent before deletion.
   */
  private deleteTaskRecursive(id: string): void {
    const task = this.repository.findById(id);
    if (task === undefined) return;

    // Delete all subtasks first (depth-first)
    for (const childId of [...task.subtaskIds]) {
      this.deleteTaskRecursive(childId);
    }

    // Unlink from parent
    if (task.parentId !== null) {
      this.unlinkChildFromParent(id, task.parentId);
    }

    this.repository.delete(id);
  }

  /**
   * Collects the set of all descendant ids of `taskId` (not including itself).
   *
   * Used for cycle detection in {@link moveTask}.
   */
  private collectDescendantIds(taskId: string): Set<string> {
    const result = new Set<string>();
    const queue: string[] = [];
    const task = this.repository.findById(taskId);
    if (task !== undefined) {
      queue.push(...task.subtaskIds);
    }
    while (queue.length > 0) {
      const id = queue.shift()!;
      if (result.has(id)) continue;
      result.add(id);
      const child = this.repository.findById(id);
      if (child !== undefined) {
        queue.push(...child.subtaskIds);
      }
    }
    return result;
  }
}

// ---------------------------------------------------------------------------
// Module-level numeric aggregation utilities
// ---------------------------------------------------------------------------

/**
 * Returns the sum of a nullable numeric field across `tasks`.
 *
 * @param tasks — Task array.
 * @param field — The field to sum.
 */
function sumField(
  tasks: Task[],
  field: "estimatedHours" | "actualHours"
): number {
  let total = 0;
  for (const task of tasks) {
    const val = task[field];
    if (val !== null) total += val;
  }
  return total;
}

/**
 * Returns the arithmetic mean of a nullable numeric field across `tasks`.
 *
 * Returns `0` when no tasks have a non-null value.
 *
 * @param tasks — Task array.
 * @param field — The field to average.
 */
function avgField(
  tasks: Task[],
  field: "estimatedHours" | "actualHours"
): number {
  let total = 0;
  let count = 0;
  for (const task of tasks) {
    const val = task[field];
    if (val !== null) {
      total += val;
      count++;
    }
  }
  return count === 0 ? 0 : total / count;
}

// ---------------------------------------------------------------------------
// Factory export
// ---------------------------------------------------------------------------

/**
 * Creates a new {@link TaskManager} with an optional injected clock.
 *
 * @param clock — Optional clock override.
 * @example
 * ```typescript
 * const manager = createTaskManager(() => new Date("2024-01-15T12:00:00Z"));
 * ```
 */
export function createTaskManager(clock?: ClockFn): TaskManager {
  return new TaskManager(clock);
}

// Re-export domain types that callers commonly need alongside the manager.
export type {
  Task,
  CreateTaskInput,
  UpdateTaskInput,
  Filter,
  Query,
  QueryResult,
  AggregateResult,
  TaskEvent,
  TaskEventType,
  TaskStats,
};
export {
  TaskNotFoundError,
  ValidationError,
  QueryError,
};
