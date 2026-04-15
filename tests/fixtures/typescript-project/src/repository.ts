/**
 * @fileoverview In-memory task repository with secondary indices.
 *
 * The repository is the single source of truth for stored tasks.  It
 * maintains several secondary indices that allow O(1) look-up by status,
 * priority, assignee, tag, and parent without scanning every record.
 *
 * The repository is not thread-safe — JavaScript is single-threaded, so no
 * locking is required for in-process use.  If you need concurrent access from
 * multiple worker threads, wrap with a message-passing adapter.
 */

import {
  type Task,
  type TaskStatus,
  type TaskPriority,
  type RepositorySnapshot,
  TaskNotFoundError,
  cloneTask,
  TASK_STATUS,
  TASK_PRIORITY,
} from "./types.js";

// ---------------------------------------------------------------------------
// Index type alias
// ---------------------------------------------------------------------------

/** A secondary index mapping a string key to a set of task ids. */
type StringIndex = Map<string, Set<string>>;

// ---------------------------------------------------------------------------
// Repository indices
// ---------------------------------------------------------------------------

/**
 * All secondary indices maintained by {@link TaskRepository}.
 *
 * Each index maps an attribute value to the set of task ids that have that
 * value.
 */
interface RepositoryIndices {
  byStatus: Map<TaskStatus, Set<string>>;
  byPriority: Map<TaskPriority, Set<string>>;
  byAssignee: StringIndex;
  byTag: StringIndex;
  byParent: StringIndex;
}

// ---------------------------------------------------------------------------
// Index maintenance helpers (module-level, pure)
// ---------------------------------------------------------------------------

/** Initialises a fresh set of empty indices. */
function createEmptyIndices(): RepositoryIndices {
  const byStatus = new Map<TaskStatus, Set<string>>();
  for (const s of TASK_STATUS) {
    byStatus.set(s, new Set<string>());
  }
  const byPriority = new Map<TaskPriority, Set<string>>();
  for (const p of TASK_PRIORITY) {
    byPriority.set(p, new Set<string>());
  }
  return {
    byStatus,
    byPriority,
    byAssignee: new Map(),
    byTag: new Map(),
    byParent: new Map(),
  };
}

/**
 * Adds a single task id to a {@link StringIndex} under `key`, creating the
 * bucket if necessary.
 */
function indexAdd(index: StringIndex, key: string, id: string): void {
  let bucket = index.get(key);
  if (bucket === undefined) {
    bucket = new Set<string>();
    index.set(key, bucket);
  }
  bucket.add(id);
}

/**
 * Removes a single task id from a {@link StringIndex} under `key`.  Removes
 * the bucket entirely when it becomes empty to keep memory bounded.
 */
function indexRemove(index: StringIndex, key: string, id: string): void {
  const bucket = index.get(key);
  if (bucket === undefined) return;
  bucket.delete(id);
  if (bucket.size === 0) {
    index.delete(key);
  }
}

/**
 * Registers a task in all secondary indices.
 *
 * @param indices — The indices object to mutate.
 * @param task    — The task to register.
 */
function indexInsert(indices: RepositoryIndices, task: Task): void {
  // Status
  const statusBucket = indices.byStatus.get(task.status);
  if (statusBucket !== undefined) {
    statusBucket.add(task.id);
  }
  // Priority
  const priorityBucket = indices.byPriority.get(task.priority);
  if (priorityBucket !== undefined) {
    priorityBucket.add(task.id);
  }
  // Assignee
  if (task.assignee !== null) {
    indexAdd(indices.byAssignee, task.assignee, task.id);
  }
  // Tags
  for (const tag of task.tags) {
    indexAdd(indices.byTag, tag, task.id);
  }
  // Parent
  if (task.parentId !== null) {
    indexAdd(indices.byParent, task.parentId, task.id);
  }
}

/**
 * Removes a task from all secondary indices.
 *
 * @param indices — The indices object to mutate.
 * @param task    — The task to de-register.
 */
function indexDelete(indices: RepositoryIndices, task: Task): void {
  // Status
  const statusBucket = indices.byStatus.get(task.status);
  if (statusBucket !== undefined) {
    statusBucket.delete(task.id);
  }
  // Priority
  const priorityBucket = indices.byPriority.get(task.priority);
  if (priorityBucket !== undefined) {
    priorityBucket.delete(task.id);
  }
  // Assignee
  if (task.assignee !== null) {
    indexRemove(indices.byAssignee, task.assignee, task.id);
  }
  // Tags
  for (const tag of task.tags) {
    indexRemove(indices.byTag, tag, task.id);
  }
  // Parent
  if (task.parentId !== null) {
    indexRemove(indices.byParent, task.parentId, task.id);
  }
}

/**
 * Updates secondary indices when a task transitions from `oldTask` to
 * `newTask`.  Only the indices that are actually affected are touched.
 *
 * @param indices — The indices object to mutate.
 * @param oldTask — The task state before the update.
 * @param newTask — The task state after the update.
 */
function indexUpdate(
  indices: RepositoryIndices,
  oldTask: Task,
  newTask: Task
): void {
  // Status
  if (oldTask.status !== newTask.status) {
    const oldBucket = indices.byStatus.get(oldTask.status);
    if (oldBucket !== undefined) oldBucket.delete(oldTask.id);
    const newBucket = indices.byStatus.get(newTask.status);
    if (newBucket !== undefined) newBucket.add(newTask.id);
  }
  // Priority
  if (oldTask.priority !== newTask.priority) {
    const oldBucket = indices.byPriority.get(oldTask.priority);
    if (oldBucket !== undefined) oldBucket.delete(oldTask.id);
    const newBucket = indices.byPriority.get(newTask.priority);
    if (newBucket !== undefined) newBucket.add(newTask.id);
  }
  // Assignee
  if (oldTask.assignee !== newTask.assignee) {
    if (oldTask.assignee !== null) {
      indexRemove(indices.byAssignee, oldTask.assignee, oldTask.id);
    }
    if (newTask.assignee !== null) {
      indexAdd(indices.byAssignee, newTask.assignee, newTask.id);
    }
  }
  // Tags — diff the two sets
  const oldTags = new Set(oldTask.tags);
  const newTags = new Set(newTask.tags);
  for (const tag of oldTags) {
    if (!newTags.has(tag)) {
      indexRemove(indices.byTag, tag, oldTask.id);
    }
  }
  for (const tag of newTags) {
    if (!oldTags.has(tag)) {
      indexAdd(indices.byTag, tag, newTask.id);
    }
  }
  // Parent
  if (oldTask.parentId !== newTask.parentId) {
    if (oldTask.parentId !== null) {
      indexRemove(indices.byParent, oldTask.parentId, oldTask.id);
    }
    if (newTask.parentId !== null) {
      indexAdd(indices.byParent, newTask.parentId, newTask.id);
    }
  }
}

// ---------------------------------------------------------------------------
// TaskRepository class
// ---------------------------------------------------------------------------

/**
 * An in-memory, index-backed store for {@link Task} records.
 *
 * All read methods return deep clones so that callers cannot accidentally
 * mutate stored state.  All write methods accept live `Task` objects but
 * clone them internally before storage.
 *
 * @example
 * ```typescript
 * const repo = new TaskRepository();
 * repo.insert(myTask);
 * const task = repo.findById(myTask.id); // returns a clone
 * ```
 */
export class TaskRepository {
  /** The primary storage map: task id → deep-cloned Task. */
  private readonly records: Map<string, Task>;

  /** Secondary indices for O(1) filtered look-ups. */
  private readonly indices: RepositoryIndices;

  /**
   * Creates an empty repository.
   */
  constructor() {
    this.records = new Map<string, Task>();
    this.indices = createEmptyIndices();
  }

  // -------------------------------------------------------------------------
  // Write operations
  // -------------------------------------------------------------------------

  /**
   * Inserts a new task into the repository.
   *
   * The task is deep-cloned before storage so that subsequent mutations of
   * the caller's object do not affect the stored record.
   *
   * @param task — The task to insert.
   * @throws {Error} When a task with the same id already exists.
   */
  insert(task: Task): void {
    if (this.records.has(task.id)) {
      throw new Error(
        `TaskRepository: duplicate id "${task.id}". Use update() to modify an existing record.`
      );
    }
    const stored = cloneTask(task);
    this.records.set(task.id, stored);
    indexInsert(this.indices, stored);
  }

  /**
   * Applies `patch` to the stored task with the given `id` and returns a
   * deep clone of the updated record.
   *
   * Only fields present in `patch` are changed.  The `id` and `createdAt`
   * fields cannot be changed through this method.
   *
   * @param id    — The id of the task to update.
   * @param patch — A partial task containing the fields to overwrite.
   * @returns The updated task (deep clone).
   * @throws {@link TaskNotFoundError} When no task with `id` exists.
   */
  update(id: string, patch: Partial<Task>): Task {
    const existing = this.records.get(id);
    if (existing === undefined) {
      throw new TaskNotFoundError(id);
    }
    const oldSnapshot = cloneTask(existing);
    // Apply the patch to the stored record in-place (it's already our own clone).
    const updated: Task = { ...existing, ...patch, id: existing.id, createdAt: existing.createdAt };
    this.records.set(id, updated);
    indexUpdate(this.indices, oldSnapshot, updated);
    return cloneTask(updated);
  }

  /**
   * Removes the task with the given `id` from the repository.
   *
   * @param id — The id of the task to remove.
   * @throws {@link TaskNotFoundError} When no task with `id` exists.
   */
  delete(id: string): void {
    const existing = this.records.get(id);
    if (existing === undefined) {
      throw new TaskNotFoundError(id);
    }
    indexDelete(this.indices, existing);
    this.records.delete(id);
  }

  // -------------------------------------------------------------------------
  // Read operations — single record
  // -------------------------------------------------------------------------

  /**
   * Returns a deep clone of the task with the given `id`, or `undefined` when
   * no task with that id exists.
   *
   * @param id — The task id to look up.
   */
  findById(id: string): Task | undefined {
    const task = this.records.get(id);
    return task !== undefined ? cloneTask(task) : undefined;
  }

  // -------------------------------------------------------------------------
  // Read operations — collections
  // -------------------------------------------------------------------------

  /**
   * Returns deep clones of all stored tasks in insertion order.
   */
  findAll(): Task[] {
    return Array.from(this.records.values()).map(cloneTask);
  }

  /**
   * Returns deep clones of all tasks whose `status` equals `status`.
   *
   * Uses the `byStatus` index for O(|result|) performance.
   *
   * @param status — The status to filter by.
   */
  findByStatus(status: TaskStatus): Task[] {
    const bucket = this.indices.byStatus.get(status);
    if (bucket === undefined) return [];
    return this.resolveIds(bucket);
  }

  /**
   * Returns deep clones of all tasks whose `priority` equals `priority`.
   *
   * Uses the `byPriority` index for O(|result|) performance.
   *
   * @param priority — The priority to filter by.
   */
  findByPriority(priority: TaskPriority): Task[] {
    const bucket = this.indices.byPriority.get(priority);
    if (bucket === undefined) return [];
    return this.resolveIds(bucket);
  }

  /**
   * Returns deep clones of all tasks assigned to `assignee`.
   *
   * Uses the `byAssignee` index for O(|result|) performance.
   *
   * @param assignee — The assignee username / user-id to filter by.
   */
  findByAssignee(assignee: string): Task[] {
    const bucket = this.indices.byAssignee.get(assignee);
    if (bucket === undefined) return [];
    return this.resolveIds(bucket);
  }

  /**
   * Returns deep clones of all tasks that have `tag` in their `tags` array.
   *
   * Uses the `byTag` index for O(|result|) performance.
   *
   * @param tag — The tag string to filter by.
   */
  findByTag(tag: string): Task[] {
    const bucket = this.indices.byTag.get(tag);
    if (bucket === undefined) return [];
    return this.resolveIds(bucket);
  }

  /**
   * Returns deep clones of all tasks whose `parentId` equals `parentId`.
   *
   * Uses the `byParent` index for O(|result|) performance.
   *
   * @param parentId — The parent task id to look up children for.
   */
  findByParent(parentId: string): Task[] {
    const bucket = this.indices.byParent.get(parentId);
    if (bucket === undefined) return [];
    return this.resolveIds(bucket);
  }

  // -------------------------------------------------------------------------
  // Aggregation helpers
  // -------------------------------------------------------------------------

  /**
   * Returns the total number of tasks currently stored.
   */
  size(): number {
    return this.records.size;
  }

  /**
   * Returns a map of status → count for all tasks.
   *
   * Each entry reads directly from the index bucket sizes, so this is O(|statuses|).
   */
  countByStatus(): Record<TaskStatus, number> {
    const result = {} as Record<TaskStatus, number>;
    for (const s of TASK_STATUS) {
      result[s] = this.indices.byStatus.get(s)?.size ?? 0;
    }
    return result;
  }

  /**
   * Returns a map of priority → count for all tasks.
   */
  countByPriority(): Record<TaskPriority, number> {
    const result = {} as Record<TaskPriority, number>;
    for (const p of TASK_PRIORITY) {
      result[p] = this.indices.byPriority.get(p)?.size ?? 0;
    }
    return result;
  }

  /**
   * Returns the set of all unique assignee names currently indexed.
   */
  allAssignees(): string[] {
    return Array.from(this.indices.byAssignee.keys());
  }

  /**
   * Returns the set of all unique tag strings currently indexed.
   */
  allTags(): string[] {
    return Array.from(this.indices.byTag.keys());
  }

  // -------------------------------------------------------------------------
  // Repository management
  // -------------------------------------------------------------------------

  /**
   * Removes all tasks and resets all indices to empty.
   */
  clear(): void {
    this.records.clear();
    for (const bucket of this.indices.byStatus.values()) {
      bucket.clear();
    }
    for (const bucket of this.indices.byPriority.values()) {
      bucket.clear();
    }
    this.indices.byAssignee.clear();
    this.indices.byTag.clear();
    this.indices.byParent.clear();
  }

  /**
   * Checks whether a task with the given `id` exists.
   *
   * @param id — The task id to check.
   */
  has(id: string): boolean {
    return this.records.has(id);
  }

  // -------------------------------------------------------------------------
  // Snapshot / restore (for transaction support)
  // -------------------------------------------------------------------------

  /**
   * Creates a full snapshot of the current repository state.
   *
   * The snapshot is a `Map<string, Task>` where every task is deep-cloned.
   * Pass the returned value to {@link restore} to roll back to this point.
   *
   * @returns An immutable snapshot of the repository.
   */
  snapshot(): RepositorySnapshot {
    const snap = new Map<string, Task>();
    for (const [id, task] of this.records) {
      snap.set(id, cloneTask(task));
    }
    return snap;
  }

  /**
   * Restores the repository to the state captured in `snap`.
   *
   * All current records and indices are discarded.  The snapshot's tasks are
   * deep-cloned before storage so that the caller's snapshot object is not
   * modified by future operations.
   *
   * @param snap — A snapshot previously returned by {@link snapshot}.
   */
  restore(snap: RepositorySnapshot): void {
    this.clear();
    for (const task of snap.values()) {
      const stored = cloneTask(task);
      this.records.set(stored.id, stored);
      indexInsert(this.indices, stored);
    }
  }

  // -------------------------------------------------------------------------
  // Bulk read (for filter engine)
  // -------------------------------------------------------------------------

  /**
   * Exposes an iterator over all stored (cloned) tasks.
   *
   * Use this when the caller needs to iterate without materialising all tasks
   * into an array first (e.g. streaming to a filter).  Note that the iterator
   * allocates a clone per task.
   */
  *iterate(): IterableIterator<Task> {
    for (const task of this.records.values()) {
      yield cloneTask(task);
    }
  }

  /**
   * Returns the raw (uncloned) internal task for read-only internal usage.
   *
   * INTERNAL USE ONLY — do not expose to external callers.
   * The filter engine uses this to avoid clone overhead during filtering.
   *
   * @internal
   */
  _getRaw(id: string): Task | undefined {
    return this.records.get(id);
  }

  /**
   * Returns all raw (uncloned) tasks for internal scanning.
   *
   * INTERNAL USE ONLY.
   *
   * @internal
   */
  _allRaw(): IterableIterator<Task> {
    return this.records.values();
  }

  // -------------------------------------------------------------------------
  // Private helpers
  // -------------------------------------------------------------------------

  /**
   * Resolves an index bucket (set of ids) to an array of deep-cloned tasks,
   * skipping any ids that are no longer in the primary map (which should never
   * happen in a consistent repository, but is a safety net).
   */
  private resolveIds(ids: Set<string>): Task[] {
    const result: Task[] = [];
    for (const id of ids) {
      const task = this.records.get(id);
      if (task !== undefined) {
        result.push(cloneTask(task));
      }
    }
    return result;
  }
}

// ---------------------------------------------------------------------------
// Factory helpers
// ---------------------------------------------------------------------------

/**
 * Creates a new empty {@link TaskRepository}.
 *
 * This is purely a convenience alias so callers don't have to import the
 * class name separately.
 *
 * @example
 * ```typescript
 * import { createRepository } from "./repository.js";
 * const repo = createRepository();
 * ```
 */
export function createRepository(): TaskRepository {
  return new TaskRepository();
}

/**
 * Populates a repository from an array of tasks, inserting each one.
 *
 * @param tasks — The tasks to bulk-load.
 * @returns The populated repository.
 */
export function repositoryFromTasks(tasks: Task[]): TaskRepository {
  const repo = new TaskRepository();
  for (const task of tasks) {
    repo.insert(task);
  }
  return repo;
}

/**
 * Returns a deep clone of the entire repository as a plain `Task[]`.
 *
 * Equivalent to `repo.findAll()` but named to make the intent explicit when
 * used in export / serialisation flows.
 *
 * @param repo — The repository to dump.
 */
export function dumpRepository(repo: TaskRepository): Task[] {
  return repo.findAll();
}

// ---------------------------------------------------------------------------
// Repository statistics helper
// ---------------------------------------------------------------------------

/**
 * Computes a summary object describing the distribution of tasks across
 * status and priority buckets.
 *
 * @param repo — The repository to summarise.
 * @param now  — Reference timestamp for overdue calculation.
 */
export function computeRepositoryStats(
  repo: TaskRepository,
  now: Date = new Date()
): {
  total: number;
  byStatus: Record<TaskStatus, number>;
  byPriority: Record<TaskPriority, number>;
  overdue: number;
} {
  const total = repo.size();
  const byStatus = repo.countByStatus();
  const byPriority = repo.countByPriority();
  let overdue = 0;
  for (const task of repo._allRaw()) {
    if (
      task.dueDate !== null &&
      task.dueDate.getTime() < now.getTime() &&
      task.status !== "done" &&
      task.status !== "cancelled"
    ) {
      overdue++;
    }
  }
  return { total, byStatus, byPriority, overdue };
}

// ---------------------------------------------------------------------------
// Index inspection (for debugging / tests)
// ---------------------------------------------------------------------------

/**
 * Returns a plain object describing the current state of all secondary
 * indices.  Useful for debugging and test assertions.
 *
 * @param repo — The repository to inspect.
 */
export function inspectIndices(repo: TaskRepository): {
  byStatus: Record<string, string[]>;
  byPriority: Record<string, string[]>;
  byAssignee: Record<string, string[]>;
  byTag: Record<string, string[]>;
  byParent: Record<string, string[]>;
} {
  // We access private state via the snapshot mechanism to keep this pure.
  const snap = repo.snapshot();
  const byStatus: Record<string, string[]> = {};
  const byPriority: Record<string, string[]> = {};
  const byAssignee: Record<string, string[]> = {};
  const byTag: Record<string, string[]> = {};
  const byParent: Record<string, string[]> = {};

  for (const s of TASK_STATUS) {
    byStatus[s] = [];
  }
  for (const p of TASK_PRIORITY) {
    byPriority[p] = [];
  }
  for (const task of snap.values()) {
    const status = byStatus[task.status];
    if (status !== undefined) status.push(task.id);
    const priority = byPriority[task.priority];
    if (priority !== undefined) priority.push(task.id);
    if (task.assignee !== null) {
      if (byAssignee[task.assignee] === undefined) byAssignee[task.assignee] = [];
      byAssignee[task.assignee]!.push(task.id);
    }
    for (const tag of task.tags) {
      if (byTag[tag] === undefined) byTag[tag] = [];
      byTag[tag]!.push(task.id);
    }
    if (task.parentId !== null) {
      if (byParent[task.parentId] === undefined) byParent[task.parentId] = [];
      byParent[task.parentId]!.push(task.id);
    }
  }
  return { byStatus, byPriority, byAssignee, byTag, byParent };
}

// ---------------------------------------------------------------------------
// Additional repository utilities
// ---------------------------------------------------------------------------

/**
 * Returns a sorted array of tasks from `repo` ordered by `createdAt`
 * ascending (oldest first).
 *
 * @param repo — The repository to query.
 */
export function findAllByCreatedAtAsc(repo: TaskRepository): Task[] {
  return repo.findAll().sort(
    (a, b) => a.createdAt.getTime() - b.createdAt.getTime()
  );
}

/**
 * Returns a sorted array of tasks from `repo` ordered by `updatedAt`
 * descending (most recently updated first).
 *
 * @param repo — The repository to query.
 */
export function findAllByUpdatedAtDesc(repo: TaskRepository): Task[] {
  return repo.findAll().sort(
    (a, b) => b.updatedAt.getTime() - a.updatedAt.getTime()
  );
}

/**
 * Returns all tasks in `repo` that have at least one subtask (i.e. their
 * `subtaskIds` array is non-empty).
 *
 * @param repo — The repository to query.
 */
export function findParentTasks(repo: TaskRepository): Task[] {
  return repo.findAll().filter((t) => t.subtaskIds.length > 0);
}

/**
 * Returns all root tasks (tasks whose `parentId` is `null`).
 *
 * @param repo — The repository to query.
 */
export function findRootTasks(repo: TaskRepository): Task[] {
  return repo.findAll().filter((t) => t.parentId === null);
}

/**
 * Returns all tasks in `repo` that are overdue relative to `now`.
 *
 * A task is overdue when its `dueDate` is in the past and its status is
 * neither `"done"` nor `"cancelled"`.
 *
 * @param repo — The repository to query.
 * @param now  — Reference time; defaults to `new Date()`.
 */
export function findOverdueTasks(
  repo: TaskRepository,
  now: Date = new Date()
): Task[] {
  return repo.findAll().filter(
    (t) =>
      t.dueDate !== null &&
      t.dueDate.getTime() < now.getTime() &&
      t.status !== "done" &&
      t.status !== "cancelled"
  );
}

/**
 * Returns tasks in `repo` whose `estimatedHours` falls within the closed
 * interval `[min, max]`.
 *
 * Tasks with `estimatedHours = null` are excluded.
 *
 * @param repo — The repository to query.
 * @param min  — Lower bound (inclusive).
 * @param max  — Upper bound (inclusive).
 */
export function findByEstimatedHoursRange(
  repo: TaskRepository,
  min: number,
  max: number
): Task[] {
  return repo.findAll().filter(
    (t) =>
      t.estimatedHours !== null &&
      t.estimatedHours >= min &&
      t.estimatedHours <= max
  );
}

/**
 * Returns tasks in `repo` that have ALL of the specified tags.
 *
 * @param repo — The repository to query.
 * @param tags — Tag strings that every returned task must have.
 */
export function findByAllTags(repo: TaskRepository, tags: string[]): Task[] {
  if (tags.length === 0) return repo.findAll();
  return repo.findAll().filter((t) => tags.every((tag) => t.tags.includes(tag)));
}

/**
 * Returns the task with the most-recent `createdAt` timestamp, or `undefined`
 * when the repository is empty.
 *
 * @param repo — The repository to query.
 */
export function findLatestCreated(repo: TaskRepository): Task | undefined {
  const all = repo.findAll();
  if (all.length === 0) return undefined;
  let latest = all[0]!;
  for (const task of all) {
    if (task.createdAt.getTime() > latest.createdAt.getTime()) {
      latest = task;
    }
  }
  return latest;
}

/**
 * Returns the task with the earliest `dueDate`, or `undefined` when no tasks
 * have a due date.
 *
 * @param repo — The repository to query.
 */
export function findEarliestDue(repo: TaskRepository): Task | undefined {
  const withDue = repo.findAll().filter((t) => t.dueDate !== null);
  if (withDue.length === 0) return undefined;
  let earliest = withDue[0]!;
  for (const task of withDue) {
    if (task.dueDate!.getTime() < earliest.dueDate!.getTime()) {
      earliest = task;
    }
  }
  return earliest;
}

/**
 * Counts tasks in `repo` grouped by assignee.
 *
 * Unassigned tasks are grouped under the key `"(unassigned)"`.
 *
 * @param repo — The repository to query.
 * @returns A map of assignee → count.
 */
export function countByAssignee(repo: TaskRepository): Map<string, number> {
  const result = new Map<string, number>();
  for (const task of repo.findAll()) {
    const key = task.assignee ?? "(unassigned)";
    result.set(key, (result.get(key) ?? 0) + 1);
  }
  return result;
}

/**
 * Counts tasks in `repo` grouped by tag.
 *
 * Tasks with no tags do not contribute to any bucket.
 *
 * @param repo — The repository to query.
 * @returns A map of tag → count.
 */
export function countByTag(repo: TaskRepository): Map<string, number> {
  const result = new Map<string, number>();
  for (const task of repo.findAll()) {
    for (const tag of task.tags) {
      result.set(tag, (result.get(tag) ?? 0) + 1);
    }
  }
  return result;
}

/**
 * Returns the total estimated hours across all tasks in `repo` that match
 * `status`.
 *
 * @param repo   — The repository to query.
 * @param status — The status bucket to sum.
 */
export function totalEstimatedHoursByStatus(
  repo: TaskRepository,
  status: import("./types.js").TaskStatus
): number {
  return repo
    .findByStatus(status)
    .reduce((acc, t) => acc + (t.estimatedHours ?? 0), 0);
}

/**
 * Returns the total actual hours across all tasks in `repo` that are assigned
 * to `assignee`.
 *
 * @param repo     — The repository to query.
 * @param assignee — The assignee to sum hours for.
 */
export function totalActualHoursByAssignee(
  repo: TaskRepository,
  assignee: string
): number {
  return repo
    .findByAssignee(assignee)
    .reduce((acc, t) => acc + (t.actualHours ?? 0), 0);
}

/**
 * Merges `source` into `target`, inserting tasks from `source` that do not
 * already exist in `target`.  Tasks that exist in both repositories are
 * left untouched in `target`.
 *
 * @param target — The repository to merge into.
 * @param source — The repository to copy from.
 * @returns The number of tasks inserted from `source`.
 */
export function mergeRepositories(
  target: TaskRepository,
  source: TaskRepository
): number {
  let count = 0;
  for (const task of source.findAll()) {
    if (!target.has(task.id)) {
      target.insert(task);
      count++;
    }
  }
  return count;
}

/**
 * Returns `true` when every task id referenced in `subtaskIds` of any task in
 * `repo` also exists as a top-level record in `repo`.
 *
 * This is a structural integrity check; a well-formed repository should
 * always satisfy it.
 *
 * @param repo — The repository to validate.
 */
export function isSubtaskReferenceIntact(repo: TaskRepository): boolean {
  for (const task of repo.findAll()) {
    for (const childId of task.subtaskIds) {
      if (!repo.has(childId)) return false;
    }
    if (task.parentId !== null && !repo.has(task.parentId)) return false;
  }
  return true;
}

/**
 * Returns the set of "orphaned" task ids — tasks whose `parentId` points to a
 * non-existent task.
 *
 * @param repo — The repository to inspect.
 */
export function findOrphanedTasks(repo: TaskRepository): Task[] {
  return repo.findAll().filter(
    (t) => t.parentId !== null && !repo.has(t.parentId)
  );
}

/**
 * Returns a deduplicated list of all unique priority values present in the
 * repository (may be fewer than 4 if some priorities are unused).
 *
 * @param repo — The repository to query.
 */
export function presentPriorities(
  repo: TaskRepository
): import("./types.js").TaskPriority[] {
  const seen = new Set<import("./types.js").TaskPriority>();
  for (const task of repo.findAll()) seen.add(task.priority);
  return Array.from(seen);
}

/**
 * Returns a deduplicated list of all unique status values present in the
 * repository.
 *
 * @param repo — The repository to query.
 */
export function presentStatuses(
  repo: TaskRepository
): import("./types.js").TaskStatus[] {
  const seen = new Set<import("./types.js").TaskStatus>();
  for (const task of repo.findAll()) seen.add(task.status);
  return Array.from(seen);
}
