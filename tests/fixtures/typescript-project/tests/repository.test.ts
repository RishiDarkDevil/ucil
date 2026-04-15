import { describe, it, expect, beforeEach } from "vitest";
import {
  TaskRepository,
  createRepository,
  repositoryFromTasks,
  inspectIndices,
} from "../src/repository.js";
import type { Task } from "../src/types.js";

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

let counter = 0;

function makeTask(overrides: Partial<Task> = {}): Task {
  counter++;
  const now = new Date("2024-06-01T00:00:00.000Z");
  return {
    id: `t${counter}`,
    title: `Task ${counter}`,
    description: "",
    status: "pending",
    priority: "medium",
    tags: [],
    assignee: null,
    dueDate: null,
    createdAt: now,
    updatedAt: now,
    parentId: null,
    subtaskIds: [],
    metadata: {},
    estimatedHours: null,
    actualHours: null,
    ...overrides,
  };
}

describe("TaskRepository", () => {
  let repo: TaskRepository;

  beforeEach(() => {
    repo = new TaskRepository();
    counter = 0;
  });

  // -------------------------------------------------------------------------
  // Basic insert / retrieve
  // -------------------------------------------------------------------------

  it("inserts and retrieves a task by id", () => {
    const task = makeTask({ title: "Hello world" });
    repo.insert(task);
    const found = repo.findById(task.id);
    expect(found).toBeDefined();
    expect(found?.id).toBe(task.id);
    expect(found?.title).toBe("Hello world");
  });

  it("returns undefined for unknown id", () => {
    expect(repo.findById("ghost")).toBeUndefined();
  });

  it("throws on duplicate insert", () => {
    const task = makeTask();
    repo.insert(task);
    expect(() => repo.insert(task)).toThrow();
  });

  it("findById returns a clone, not the stored reference", () => {
    const task = makeTask({ title: "Original" });
    repo.insert(task);
    const clone1 = repo.findById(task.id)!;
    clone1.title = "Mutated";
    const clone2 = repo.findById(task.id)!;
    expect(clone2.title).toBe("Original");
  });

  // -------------------------------------------------------------------------
  // findAll
  // -------------------------------------------------------------------------

  it("findAll returns all stored tasks", () => {
    repo.insert(makeTask());
    repo.insert(makeTask());
    repo.insert(makeTask());
    const all = repo.findAll();
    expect(all.length).toBe(3);
  });

  it("findAll returns empty array when repository is empty", () => {
    expect(repo.findAll()).toEqual([]);
  });

  // -------------------------------------------------------------------------
  // Update
  // -------------------------------------------------------------------------

  it("update mutates the specified fields", () => {
    const task = makeTask({ status: "pending" });
    repo.insert(task);
    const updated = repo.update(task.id, { status: "done", priority: "high" });
    expect(updated.status).toBe("done");
    expect(updated.priority).toBe("high");
    // Verify the stored record was also updated
    expect(repo.findById(task.id)?.status).toBe("done");
  });

  it("update does not change id or createdAt", () => {
    const task = makeTask();
    repo.insert(task);
    const originalCreatedAt = task.createdAt;
    const updated = repo.update(task.id, {
      id: "hijacked-id",
      createdAt: new Date("2000-01-01"),
    } as Partial<Task>);
    expect(updated.id).toBe(task.id);
    expect(updated.createdAt.getTime()).toBe(originalCreatedAt.getTime());
  });

  it("update throws TaskNotFoundError for unknown id", () => {
    expect(() => repo.update("ghost", { title: "nope" })).toThrow();
  });

  // -------------------------------------------------------------------------
  // Delete
  // -------------------------------------------------------------------------

  it("delete removes the task from the repository", () => {
    const task = makeTask();
    repo.insert(task);
    repo.delete(task.id);
    expect(repo.findById(task.id)).toBeUndefined();
    expect(repo.size()).toBe(0);
  });

  it("delete throws TaskNotFoundError for unknown id", () => {
    expect(() => repo.delete("ghost")).toThrow();
  });

  // -------------------------------------------------------------------------
  // Status index maintenance
  // -------------------------------------------------------------------------

  it("maintains status index on insert", () => {
    repo.insert(makeTask({ status: "pending" }));
    repo.insert(makeTask({ status: "pending" }));
    repo.insert(makeTask({ status: "done" }));

    expect(repo.findByStatus("pending").length).toBe(2);
    expect(repo.findByStatus("done").length).toBe(1);
    expect(repo.findByStatus("cancelled").length).toBe(0);
  });

  it("maintains status index on update", () => {
    const task = makeTask({ status: "pending" });
    repo.insert(task);
    expect(repo.findByStatus("pending").length).toBe(1);
    expect(repo.findByStatus("in_progress").length).toBe(0);

    repo.update(task.id, { status: "in_progress" });

    expect(repo.findByStatus("pending").length).toBe(0);
    expect(repo.findByStatus("in_progress").length).toBe(1);
  });

  it("maintains status index on delete", () => {
    const task = makeTask({ status: "pending" });
    repo.insert(task);
    expect(repo.findByStatus("pending").length).toBe(1);
    repo.delete(task.id);
    expect(repo.findByStatus("pending").length).toBe(0);
  });

  // -------------------------------------------------------------------------
  // Priority index maintenance
  // -------------------------------------------------------------------------

  it("maintains priority index on insert", () => {
    repo.insert(makeTask({ priority: "high" }));
    repo.insert(makeTask({ priority: "critical" }));
    repo.insert(makeTask({ priority: "high" }));

    expect(repo.findByPriority("high").length).toBe(2);
    expect(repo.findByPriority("critical").length).toBe(1);
  });

  it("maintains priority index on update", () => {
    const task = makeTask({ priority: "low" });
    repo.insert(task);
    repo.update(task.id, { priority: "critical" });
    expect(repo.findByPriority("low").length).toBe(0);
    expect(repo.findByPriority("critical").length).toBe(1);
  });

  // -------------------------------------------------------------------------
  // Assignee index
  // -------------------------------------------------------------------------

  it("maintains assignee index on insert and delete", () => {
    const task = makeTask({ assignee: "alice" });
    repo.insert(task);
    expect(repo.findByAssignee("alice").length).toBe(1);
    repo.delete(task.id);
    expect(repo.findByAssignee("alice").length).toBe(0);
  });

  it("findByAssignee returns empty array for unknown assignee", () => {
    expect(repo.findByAssignee("nobody")).toEqual([]);
  });

  it("maintains assignee index when assignee changes", () => {
    const task = makeTask({ assignee: "alice" });
    repo.insert(task);
    repo.update(task.id, { assignee: "bob" });
    expect(repo.findByAssignee("alice").length).toBe(0);
    expect(repo.findByAssignee("bob").length).toBe(1);
  });

  // -------------------------------------------------------------------------
  // Tag index
  // -------------------------------------------------------------------------

  it("maintains tag index on insert", () => {
    repo.insert(makeTask({ tags: ["urgent", "backend"] }));
    repo.insert(makeTask({ tags: ["urgent", "frontend"] }));
    repo.insert(makeTask({ tags: ["design"] }));

    expect(repo.findByTag("urgent").length).toBe(2);
    expect(repo.findByTag("backend").length).toBe(1);
    expect(repo.findByTag("design").length).toBe(1);
    expect(repo.findByTag("missing").length).toBe(0);
  });

  it("maintains tag index when tags change on update", () => {
    const task = makeTask({ tags: ["alpha", "beta"] });
    repo.insert(task);
    repo.update(task.id, { tags: ["beta", "gamma"] });
    expect(repo.findByTag("alpha").length).toBe(0);
    expect(repo.findByTag("beta").length).toBe(1);
    expect(repo.findByTag("gamma").length).toBe(1);
  });

  // -------------------------------------------------------------------------
  // Parent index
  // -------------------------------------------------------------------------

  it("maintains parent index on insert and delete", () => {
    const parent = makeTask({ id: "parent-1" });
    const child = makeTask({ parentId: "parent-1" });
    repo.insert(parent);
    repo.insert(child);

    expect(repo.findByParent("parent-1").length).toBe(1);
    expect(repo.findByParent("parent-1")[0]?.id).toBe(child.id);

    repo.delete(child.id);
    expect(repo.findByParent("parent-1").length).toBe(0);
  });

  // -------------------------------------------------------------------------
  // Size / has / clear
  // -------------------------------------------------------------------------

  it("size returns the number of stored tasks", () => {
    expect(repo.size()).toBe(0);
    repo.insert(makeTask());
    expect(repo.size()).toBe(1);
    repo.insert(makeTask());
    expect(repo.size()).toBe(2);
  });

  it("has returns true for existing id and false otherwise", () => {
    const task = makeTask();
    repo.insert(task);
    expect(repo.has(task.id)).toBe(true);
    expect(repo.has("nope")).toBe(false);
  });

  it("clear removes all tasks and resets indices", () => {
    repo.insert(makeTask({ status: "pending", tags: ["tag1"] }));
    repo.insert(makeTask({ status: "done", assignee: "alice" }));
    expect(repo.size()).toBe(2);

    repo.clear();
    expect(repo.size()).toBe(0);
    expect(repo.findAll()).toEqual([]);
    expect(repo.findByStatus("pending")).toEqual([]);
    expect(repo.findByAssignee("alice")).toEqual([]);
  });

  // -------------------------------------------------------------------------
  // countByStatus / countByPriority
  // -------------------------------------------------------------------------

  it("countByStatus returns correct counts for all statuses", () => {
    repo.insert(makeTask({ status: "pending" }));
    repo.insert(makeTask({ status: "pending" }));
    repo.insert(makeTask({ status: "done" }));

    const counts = repo.countByStatus();
    expect(counts.pending).toBe(2);
    expect(counts.done).toBe(1);
    expect(counts.in_progress).toBe(0);
    expect(counts.blocked).toBe(0);
    expect(counts.cancelled).toBe(0);
  });

  it("countByPriority returns correct counts for all priorities", () => {
    repo.insert(makeTask({ priority: "low" }));
    repo.insert(makeTask({ priority: "high" }));
    repo.insert(makeTask({ priority: "high" }));

    const counts = repo.countByPriority();
    expect(counts.low).toBe(1);
    expect(counts.high).toBe(2);
    expect(counts.medium).toBe(0);
    expect(counts.critical).toBe(0);
  });

  // -------------------------------------------------------------------------
  // allAssignees / allTags
  // -------------------------------------------------------------------------

  it("allAssignees returns unique assignee names", () => {
    repo.insert(makeTask({ assignee: "alice" }));
    repo.insert(makeTask({ assignee: "bob" }));
    repo.insert(makeTask({ assignee: "alice" }));

    const assignees = repo.allAssignees().sort();
    expect(assignees).toEqual(["alice", "bob"]);
  });

  it("allTags returns unique tag strings", () => {
    repo.insert(makeTask({ tags: ["a", "b"] }));
    repo.insert(makeTask({ tags: ["b", "c"] }));

    const tags = repo.allTags().sort();
    expect(tags).toEqual(["a", "b", "c"]);
  });

  // -------------------------------------------------------------------------
  // Snapshot / restore
  // -------------------------------------------------------------------------

  it("snapshot and restore works correctly", () => {
    const task = makeTask({ title: "Original" });
    repo.insert(task);
    const snap = repo.snapshot();

    repo.update(task.id, { title: "Modified" });
    expect(repo.findById(task.id)?.title).toBe("Modified");

    repo.restore(snap);
    expect(repo.findById(task.id)?.title).toBe("Original");
  });

  it("restoring snapshot resets secondary indices", () => {
    const task = makeTask({ status: "pending" });
    repo.insert(task);
    const snap = repo.snapshot();

    repo.update(task.id, { status: "done" });
    expect(repo.findByStatus("done").length).toBe(1);
    expect(repo.findByStatus("pending").length).toBe(0);

    repo.restore(snap);
    expect(repo.findByStatus("pending").length).toBe(1);
    expect(repo.findByStatus("done").length).toBe(0);
  });

  it("snapshot is independent of subsequent mutations", () => {
    const task = makeTask({ title: "Snapshot test" });
    repo.insert(task);
    const snap = repo.snapshot();

    // Mutate after snapshot
    repo.update(task.id, { title: "After snapshot" });

    // The snapshot should still hold the original title
    const snapTask = snap.get(task.id);
    expect(snapTask?.title).toBe("Snapshot test");
  });

  it("restore from empty snapshot clears the repository", () => {
    repo.insert(makeTask());
    repo.insert(makeTask());
    expect(repo.size()).toBe(2);

    const emptySnap = new Map<string, Task>();
    repo.restore(emptySnap);
    expect(repo.size()).toBe(0);
  });

  // -------------------------------------------------------------------------
  // iterate
  // -------------------------------------------------------------------------

  it("iterate yields all stored tasks", () => {
    const t1 = makeTask();
    const t2 = makeTask();
    repo.insert(t1);
    repo.insert(t2);

    const ids: string[] = [];
    for (const task of repo.iterate()) {
      ids.push(task.id);
    }
    expect(ids.sort()).toEqual([t1.id, t2.id].sort());
  });

  // -------------------------------------------------------------------------
  // Factory helpers
  // -------------------------------------------------------------------------

  it("createRepository creates an empty repository", () => {
    const r = createRepository();
    expect(r.size()).toBe(0);
  });

  it("repositoryFromTasks pre-populates the repository", () => {
    const tasks = [makeTask(), makeTask(), makeTask()];
    const r = repositoryFromTasks(tasks);
    expect(r.size()).toBe(3);
    for (const t of tasks) {
      expect(r.findById(t.id)).toBeDefined();
    }
  });

  // -------------------------------------------------------------------------
  // inspectIndices (structural consistency)
  // -------------------------------------------------------------------------

  it("inspectIndices reflects current state correctly", () => {
    repo.insert(makeTask({ status: "pending", assignee: "alice", tags: ["t1"] }));
    repo.insert(makeTask({ status: "done", assignee: "alice", tags: ["t1", "t2"] }));

    const idx = inspectIndices(repo);
    expect(idx.byStatus["pending"]?.length).toBe(1);
    expect(idx.byStatus["done"]?.length).toBe(1);
    expect(idx.byAssignee["alice"]?.length).toBe(2);
    expect(idx.byTag["t1"]?.length).toBe(2);
    expect(idx.byTag["t2"]?.length).toBe(1);
  });

  // -------------------------------------------------------------------------
  // Multiple index consistency after complex sequence
  // -------------------------------------------------------------------------

  it("maintains index consistency across insert→update→delete sequence", () => {
    const task = makeTask({
      status: "pending",
      priority: "low",
      assignee: "carol",
      tags: ["old-tag"],
    });
    repo.insert(task);

    // Verify initial indices
    expect(repo.findByStatus("pending").length).toBe(1);
    expect(repo.findByPriority("low").length).toBe(1);
    expect(repo.findByAssignee("carol").length).toBe(1);
    expect(repo.findByTag("old-tag").length).toBe(1);

    // Update multiple indexed fields
    repo.update(task.id, {
      status: "in_progress",
      priority: "critical",
      assignee: "dan",
      tags: ["new-tag"],
    });

    // Old indices cleared
    expect(repo.findByStatus("pending").length).toBe(0);
    expect(repo.findByPriority("low").length).toBe(0);
    expect(repo.findByAssignee("carol").length).toBe(0);
    expect(repo.findByTag("old-tag").length).toBe(0);

    // New indices populated
    expect(repo.findByStatus("in_progress").length).toBe(1);
    expect(repo.findByPriority("critical").length).toBe(1);
    expect(repo.findByAssignee("dan").length).toBe(1);
    expect(repo.findByTag("new-tag").length).toBe(1);

    // Delete and verify all indices cleared
    repo.delete(task.id);
    expect(repo.findByStatus("in_progress").length).toBe(0);
    expect(repo.findByPriority("critical").length).toBe(0);
    expect(repo.findByAssignee("dan").length).toBe(0);
    expect(repo.findByTag("new-tag").length).toBe(0);
    expect(repo.size()).toBe(0);
  });
});
