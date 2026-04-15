import { describe, it, expect, beforeEach } from "vitest";
import { TaskManager } from "../src/task-manager.js";
import type { TaskEvent } from "../src/types.js";

describe("TaskManager", () => {
  let manager: TaskManager;

  beforeEach(() => {
    manager = new TaskManager();
  });

  // -------------------------------------------------------------------------
  // Creation
  // -------------------------------------------------------------------------

  it("creates a task with required fields only", () => {
    const task = manager.createTask({ title: "Write tests" });
    expect(task.id).toBeTypeOf("string");
    expect(task.id.length).toBeGreaterThan(0);
    expect(task.title).toBe("Write tests");
    expect(task.description).toBe("");
    expect(task.status).toBe("pending");
    expect(task.priority).toBe("medium");
    expect(task.tags).toEqual([]);
    expect(task.assignee).toBeNull();
    expect(task.parentId).toBeNull();
    expect(task.subtaskIds).toEqual([]);
    expect(task.metadata).toEqual({});
    expect(task.estimatedHours).toBeNull();
    expect(task.actualHours).toBeNull();
    expect(task.createdAt).toBeInstanceOf(Date);
    expect(task.updatedAt).toBeInstanceOf(Date);
  });

  it("creates a task with all optional fields provided", () => {
    const due = new Date("2025-06-01T09:00:00.000Z");
    const task = manager.createTask({
      title: "Deploy to production",
      description: "Full production deployment with rollback plan",
      status: "in_progress",
      priority: "critical",
      tags: ["deploy", "production"],
      assignee: "alice",
      dueDate: due,
      metadata: { env: "prod", version: "1.2.3" },
      estimatedHours: 4,
      actualHours: 1.5,
    });
    expect(task.title).toBe("Deploy to production");
    expect(task.description).toBe("Full production deployment with rollback plan");
    expect(task.status).toBe("in_progress");
    expect(task.priority).toBe("critical");
    expect(task.tags).toEqual(["deploy", "production"]);
    expect(task.assignee).toBe("alice");
    expect(task.dueDate?.toISOString()).toBe(due.toISOString());
    expect(task.metadata).toEqual({ env: "prod", version: "1.2.3" });
    expect(task.estimatedHours).toBe(4);
    expect(task.actualHours).toBe(1.5);
  });

  it("throws ValidationError when title is empty", () => {
    expect(() => manager.createTask({ title: "" })).toThrow();
    expect(() => manager.createTask({ title: "   " })).toThrow();
  });

  it("throws ValidationError for an invalid status", () => {
    expect(() =>
      manager.createTask({ title: "T", status: "flying" as never })
    ).toThrow();
  });

  it("throws ValidationError for a negative estimatedHours", () => {
    expect(() =>
      manager.createTask({ title: "T", estimatedHours: -1 })
    ).toThrow();
  });

  // -------------------------------------------------------------------------
  // Retrieval
  // -------------------------------------------------------------------------

  it("getTask returns the stored task", () => {
    const created = manager.createTask({ title: "Read me" });
    const retrieved = manager.getTask(created.id);
    expect(retrieved.id).toBe(created.id);
    expect(retrieved.title).toBe("Read me");
  });

  it("getTask throws TaskNotFoundError for unknown id", () => {
    expect(() => manager.getTask("nonexistent-id")).toThrow();
  });

  // -------------------------------------------------------------------------
  // Updating
  // -------------------------------------------------------------------------

  it("updateTask mutates the specified fields", () => {
    const task = manager.createTask({ title: "Old title" });
    const updated = manager.updateTask(task.id, {
      title: "New title",
      status: "in_progress",
      priority: "high",
    });
    expect(updated.title).toBe("New title");
    expect(updated.status).toBe("in_progress");
    expect(updated.priority).toBe("high");
    expect(updated.updatedAt.getTime()).toBeGreaterThanOrEqual(
      task.updatedAt.getTime()
    );
  });

  it("updateTask does not change unspecified fields", () => {
    const task = manager.createTask({
      title: "Stable task",
      assignee: "bob",
      tags: ["important"],
    });
    const updated = manager.updateTask(task.id, { priority: "low" });
    expect(updated.title).toBe("Stable task");
    expect(updated.assignee).toBe("bob");
    expect(updated.tags).toEqual(["important"]);
    expect(updated.priority).toBe("low");
  });

  // -------------------------------------------------------------------------
  // Deletion
  // -------------------------------------------------------------------------

  it("deleteTask removes the task from the store", () => {
    const task = manager.createTask({ title: "Delete me" });
    manager.deleteTask(task.id);
    expect(() => manager.getTask(task.id)).toThrow();
  });

  it("deleteTask throws TaskNotFoundError for unknown id", () => {
    expect(() => manager.deleteTask("ghost")).toThrow();
  });

  // -------------------------------------------------------------------------
  // Queries — filter by status
  // -------------------------------------------------------------------------

  it("queries tasks by status filter", () => {
    manager.createTask({ title: "P1", status: "pending" });
    manager.createTask({ title: "P2", status: "pending" });
    manager.createTask({ title: "D1", status: "done" });

    const result = manager.query({
      filter: { field: "status", operator: "eq", value: "pending" },
    });
    expect(result.total).toBe(2);
    expect(result.items.every((t) => t.status === "pending")).toBe(true);
  });

  it("findAll without filter returns every task", () => {
    manager.createTask({ title: "A" });
    manager.createTask({ title: "B" });
    manager.createTask({ title: "C" });
    expect(manager.findAll().length).toBe(3);
  });

  it("count returns the number of matching tasks", () => {
    manager.createTask({ title: "Hi", priority: "high" });
    manager.createTask({ title: "Lo", priority: "low" });
    manager.createTask({ title: "Hi2", priority: "high" });
    expect(
      manager.count({ field: "priority", operator: "eq", value: "high" })
    ).toBe(2);
    expect(
      manager.count({ field: "priority", operator: "eq", value: "low" })
    ).toBe(1);
  });

  // -------------------------------------------------------------------------
  // Aggregation
  // -------------------------------------------------------------------------

  it("aggregates tasks by priority", () => {
    manager.createTask({ title: "A", priority: "high" });
    manager.createTask({ title: "B", priority: "high" });
    manager.createTask({ title: "C", priority: "low" });

    const result = manager.aggregate("priority");
    expect(result.count).toBe(3);
    expect(result.groupBy?.["high"]?.count).toBe(2);
    expect(result.groupBy?.["low"]?.count).toBe(1);
  });

  it("sumField sums estimatedHours across filtered tasks", () => {
    manager.createTask({ title: "A", estimatedHours: 2 });
    manager.createTask({ title: "B", estimatedHours: 3 });
    manager.createTask({ title: "C", estimatedHours: null });
    expect(manager.sumField("estimatedHours")).toBe(5);
  });

  // -------------------------------------------------------------------------
  // Subtask hierarchy
  // -------------------------------------------------------------------------

  it("handles subtask hierarchy", () => {
    const parent = manager.createTask({ title: "Epic" });
    const child1 = manager.createTask({
      title: "Sub-task 1",
      parentId: parent.id,
    });
    const child2 = manager.createTask({
      title: "Sub-task 2",
      parentId: parent.id,
    });

    const refreshedParent = manager.getTask(parent.id);
    expect(refreshedParent.subtaskIds).toContain(child1.id);
    expect(refreshedParent.subtaskIds).toContain(child2.id);

    const subtasks = manager.getSubtasks(parent.id);
    expect(subtasks.length).toBe(2);
    expect(subtasks.map((t) => t.id).sort()).toEqual(
      [child1.id, child2.id].sort()
    );

    const ancestors = manager.getAncestors(child1.id);
    expect(ancestors.length).toBe(1);
    expect(ancestors[0]?.id).toBe(parent.id);
  });

  it("deleteTask on parent also removes subtasks", () => {
    const parent = manager.createTask({ title: "Parent" });
    const child = manager.createTask({
      title: "Child",
      parentId: parent.id,
    });

    manager.deleteTask(parent.id);
    expect(() => manager.getTask(parent.id)).toThrow();
    expect(() => manager.getTask(child.id)).toThrow();
  });

  it("moveTask moves a task to a new parent", () => {
    const a = manager.createTask({ title: "Parent A" });
    const b = manager.createTask({ title: "Parent B" });
    const child = manager.createTask({ title: "Child", parentId: a.id });

    const moved = manager.moveTask(child.id, b.id);
    expect(moved.parentId).toBe(b.id);

    const newParent = manager.getTask(b.id);
    expect(newParent.subtaskIds).toContain(child.id);

    const oldParent = manager.getTask(a.id);
    expect(oldParent.subtaskIds).not.toContain(child.id);
  });

  it("moveTask throws on cyclic reparenting", () => {
    const root = manager.createTask({ title: "Root" });
    const child = manager.createTask({ title: "Child", parentId: root.id });
    expect(() => manager.moveTask(root.id, child.id)).toThrow();
  });

  // -------------------------------------------------------------------------
  // Events
  // -------------------------------------------------------------------------

  it("fires 'created' event on task creation", () => {
    const events: TaskEvent[] = [];
    manager.on("created", (e) => events.push(e));

    const task = manager.createTask({ title: "Event task" });
    expect(events.length).toBe(1);
    expect(events[0]?.type).toBe("created");
    expect(events[0]?.taskId).toBe(task.id);
  });

  it("fires 'statusChanged' event when status is updated", () => {
    const task = manager.createTask({ title: "Status watcher" });
    const events: TaskEvent[] = [];
    manager.on("statusChanged", (e) => events.push(e));

    manager.updateTask(task.id, { status: "in_progress" });
    expect(events.length).toBe(1);
    expect(events[0]?.previousValue).toBe("pending");
    expect(events[0]?.newValue).toBe("in_progress");
  });

  it("fires 'deleted' event on task deletion", () => {
    const task = manager.createTask({ title: "Doomed task" });
    const events: TaskEvent[] = [];
    manager.on("deleted", (e) => events.push(e));

    manager.deleteTask(task.id);
    expect(events.length).toBe(1);
    expect(events[0]?.taskId).toBe(task.id);
  });

  it("off() stops receiving events", () => {
    const events: TaskEvent[] = [];
    const listener = (e: TaskEvent) => events.push(e);
    manager.on("created", listener);
    manager.createTask({ title: "Heard" });
    manager.off("created", listener);
    manager.createTask({ title: "Not heard" });
    expect(events.length).toBe(1);
  });

  // -------------------------------------------------------------------------
  // Bulk operations
  // -------------------------------------------------------------------------

  it("createMany inserts multiple tasks atomically", () => {
    const tasks = manager.createMany([
      { title: "Bulk 1" },
      { title: "Bulk 2" },
      { title: "Bulk 3" },
    ]);
    expect(tasks.length).toBe(3);
    expect(manager.count()).toBe(3);
  });

  it("updateMany patches all matching tasks", () => {
    manager.createTask({ title: "A", status: "pending" });
    manager.createTask({ title: "B", status: "pending" });
    manager.createTask({ title: "C", status: "done" });

    const updated = manager.updateMany(
      { field: "status", operator: "eq", value: "pending" },
      { priority: "high" }
    );
    expect(updated.length).toBe(2);
    expect(updated.every((t) => t.priority === "high")).toBe(true);
  });

  it("deleteMany removes all matching tasks", () => {
    manager.createTask({ title: "Remove me", status: "cancelled" });
    manager.createTask({ title: "Remove me too", status: "cancelled" });
    manager.createTask({ title: "Keep me", status: "pending" });

    const deleted = manager.deleteMany({
      field: "status",
      operator: "eq",
      value: "cancelled",
    });
    expect(deleted).toBe(2);
    expect(manager.count()).toBe(1);
  });

  // -------------------------------------------------------------------------
  // Import / Export
  // -------------------------------------------------------------------------

  it("exportToJSON and importFromJSON roundtrip", () => {
    const t1 = manager.createTask({ title: "Export 1", priority: "high" });
    const t2 = manager.createTask({
      title: "Export 2",
      estimatedHours: 3.5,
    });

    const json = manager.exportToJSON();
    const manager2 = new TaskManager();
    const count = manager2.importFromJSON(json);
    expect(count).toBe(2);

    const rt1 = manager2.getTask(t1.id);
    const rt2 = manager2.getTask(t2.id);
    expect(rt1.title).toBe("Export 1");
    expect(rt1.priority).toBe("high");
    expect(rt2.estimatedHours).toBe(3.5);
  });

  it("importFromJSON skips duplicate ids", () => {
    const task = manager.createTask({ title: "Unique" });
    const json = manager.exportToJSON();
    const count = manager.importFromJSON(json);
    expect(count).toBe(0); // already present
    expect(manager.getTask(task.id).title).toBe("Unique");
  });

  // -------------------------------------------------------------------------
  // Transactions
  // -------------------------------------------------------------------------

  it("transaction rolls back on error", () => {
    manager.createTask({ title: "Existing" });
    expect(manager.count()).toBe(1);

    expect(() => {
      manager.transaction(() => {
        manager.createTask({ title: "Inside tx" });
        throw new Error("forced rollback");
      });
    }).toThrow("forced rollback");

    // Still only the original task
    expect(manager.count()).toBe(1);
  });

  // -------------------------------------------------------------------------
  // Statistics
  // -------------------------------------------------------------------------

  it("getStats returns correct counts", () => {
    manager.createTask({ title: "P1" });
    manager.createTask({ title: "P2", status: "in_progress" });
    manager.createTask({ title: "D1", status: "done" });

    const stats = manager.getStats();
    expect(stats.total).toBe(3);
    expect(stats.byStatus.pending).toBe(1);
    expect(stats.byStatus.in_progress).toBe(1);
    expect(stats.byStatus.done).toBe(1);
  });

  it("getStats counts overdue tasks", () => {
    const pastDate = new Date("2020-01-01T00:00:00.000Z");
    manager.createTask({
      title: "Overdue",
      dueDate: pastDate,
      status: "pending",
    });
    manager.createTask({ title: "On time" });

    const stats = manager.getStats(new Date("2024-01-01T00:00:00.000Z"));
    expect(stats.overdue).toBe(1);
  });

  // -------------------------------------------------------------------------
  // Tag / metadata helpers
  // -------------------------------------------------------------------------

  it("addTag appends a tag to the task", () => {
    const task = manager.createTask({ title: "Tagged" });
    const updated = manager.addTag(task.id, "urgent");
    expect(updated.tags).toContain("urgent");
  });

  it("addTag is idempotent", () => {
    const task = manager.createTask({
      title: "Tagged",
      tags: ["urgent"],
    });
    const updated = manager.addTag(task.id, "urgent");
    expect(updated.tags.filter((t) => t === "urgent").length).toBe(1);
  });

  it("removeTag removes a tag from the task", () => {
    const task = manager.createTask({
      title: "Tagged",
      tags: ["urgent", "review"],
    });
    const updated = manager.removeTag(task.id, "urgent");
    expect(updated.tags).not.toContain("urgent");
    expect(updated.tags).toContain("review");
  });

  it("setMetadata sets a key on the task", () => {
    const task = manager.createTask({ title: "Meta task" });
    const updated = manager.setMetadata(task.id, "sprint", 5);
    expect(updated.metadata["sprint"]).toBe(5);
  });

  it("deleteMetadata removes a key from the task", () => {
    const task = manager.createTask({
      title: "Meta task",
      metadata: { sprint: 5, owner: "team-a" },
    });
    const updated = manager.deleteMetadata(task.id, "sprint");
    expect(updated.metadata["sprint"]).toBeUndefined();
    expect(updated.metadata["owner"]).toBe("team-a");
  });

  // -------------------------------------------------------------------------
  // Search
  // -------------------------------------------------------------------------

  it("search returns tasks matching title substring", () => {
    manager.createTask({ title: "Fix login bug" });
    manager.createTask({ title: "Add logout feature" });
    manager.createTask({ title: "Unrelated task" });

    const results = manager.search("login");
    expect(results.length).toBe(1);
    expect(results[0]?.title).toBe("Fix login bug");
  });

  it("search is case-insensitive", () => {
    manager.createTask({ title: "Deploy Service" });
    const results = manager.search("deploy");
    expect(results.length).toBe(1);
  });

  it("search matches description text", () => {
    manager.createTask({
      title: "Generic task",
      description: "Refactor the authentication module",
    });
    const results = manager.search("authentication");
    expect(results.length).toBe(1);
  });

  // -------------------------------------------------------------------------
  // Pagination
  // -------------------------------------------------------------------------

  it("paginate returns correct page slices", () => {
    for (let i = 0; i < 10; i++) {
      manager.createTask({ title: `Task ${i + 1}` });
    }
    const page1 = manager.paginate(1, 4);
    expect(page1.items.length).toBe(4);
    expect(page1.total).toBe(10);
    expect(page1.hasMore).toBe(true);

    const page3 = manager.paginate(3, 4);
    expect(page3.items.length).toBe(2); // 10 - 8 = 2 remaining
    expect(page3.hasMore).toBe(false);
  });

  // -------------------------------------------------------------------------
  // getSubtree
  // -------------------------------------------------------------------------

  it("getSubtree returns all descendants", () => {
    const root = manager.createTask({ title: "Root" });
    const child = manager.createTask({
      title: "Child",
      parentId: root.id,
    });
    manager.createTask({ title: "Grandchild", parentId: child.id });

    const subtree = manager.getSubtree(root.id);
    expect(subtree.length).toBe(3);
    expect(subtree[0]?.id).toBe(root.id);
  });

  // -------------------------------------------------------------------------
  // Status transitions
  // -------------------------------------------------------------------------

  it("transitionStatus follows allowed transitions", () => {
    const task = manager.createTask({ title: "State machine" });
    const updated = manager.transitionStatus(task.id, "in_progress");
    expect(updated.status).toBe("in_progress");
  });

  it("transitionStatus rejects disallowed transitions", () => {
    const task = manager.createTask({
      title: "Completed",
      status: "done",
    });
    expect(() => manager.transitionStatus(task.id, "pending")).toThrow();
  });

  // -------------------------------------------------------------------------
  // getPendingTasks
  // -------------------------------------------------------------------------

  it("getPendingTasks returns only pending tasks sorted by priority desc", () => {
    manager.createTask({ title: "Low prio pending", priority: "low" });
    manager.createTask({
      title: "Critical pending",
      priority: "critical",
    });
    manager.createTask({
      title: "Done task",
      status: "done",
      priority: "critical",
    });

    const pending = manager.getPendingTasks();
    expect(pending.every((t) => t.status === "pending")).toBe(true);
    expect(pending.length).toBe(2);
    // critical comes before low
    expect(pending[0]?.priority).toBe("critical");
    expect(pending[1]?.priority).toBe("low");
  });
});
