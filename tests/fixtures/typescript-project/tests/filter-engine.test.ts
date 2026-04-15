import { describe, it, expect, beforeEach } from "vitest";
import { FilterEngine, FilterExpressionParser, serializeFilter } from "../src/filter-engine.js";
import type { Task, Filter, SortSpec } from "../src/types.js";

// ---------------------------------------------------------------------------
// Helper: build a minimal valid Task
// ---------------------------------------------------------------------------

let idCounter = 0;

function makeTask(overrides: Partial<Task> = {}): Task {
  idCounter++;
  const now = new Date("2024-03-01T12:00:00.000Z");
  return {
    id: `task-${idCounter}`,
    title: `Task ${idCounter}`,
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

describe("FilterEngine", () => {
  let engine: FilterEngine;

  beforeEach(() => {
    engine = new FilterEngine();
    idCounter = 0;
  });

  // -------------------------------------------------------------------------
  // Equality
  // -------------------------------------------------------------------------

  it("applies eq filter correctly", () => {
    const tasks: Task[] = [
      makeTask({ status: "pending" }),
      makeTask({ status: "done" }),
      makeTask({ status: "pending" }),
    ];
    const filter: Filter = { field: "status", operator: "eq", value: "pending" };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(2);
    expect(result.every((t) => t.status === "pending")).toBe(true);
  });

  it("applies neq filter correctly", () => {
    const tasks: Task[] = [
      makeTask({ status: "pending" }),
      makeTask({ status: "done" }),
      makeTask({ status: "cancelled" }),
    ];
    const filter: Filter = {
      field: "status",
      operator: "neq",
      value: "pending",
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(2);
    expect(result.every((t) => t.status !== "pending")).toBe(true);
  });

  // -------------------------------------------------------------------------
  // Logical operators
  // -------------------------------------------------------------------------

  it("applies AND logical filter", () => {
    const tasks: Task[] = [
      makeTask({ status: "pending", priority: "high" }),
      makeTask({ status: "pending", priority: "low" }),
      makeTask({ status: "done", priority: "high" }),
    ];
    const filter: Filter = {
      operator: "and",
      filters: [
        { field: "status", operator: "eq", value: "pending" },
        { field: "priority", operator: "eq", value: "high" },
      ],
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(1);
    expect(result[0]?.status).toBe("pending");
    expect(result[0]?.priority).toBe("high");
  });

  it("applies OR logical filter", () => {
    const tasks: Task[] = [
      makeTask({ status: "pending" }),
      makeTask({ status: "done" }),
      makeTask({ status: "cancelled" }),
    ];
    const filter: Filter = {
      operator: "or",
      filters: [
        { field: "status", operator: "eq", value: "pending" },
        { field: "status", operator: "eq", value: "done" },
      ],
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(2);
    expect(result.every((t) => t.status === "pending" || t.status === "done")).toBe(true);
  });

  it("applies NOT logical filter", () => {
    const tasks: Task[] = [
      makeTask({ status: "pending" }),
      makeTask({ status: "done" }),
    ];
    const filter: Filter = {
      operator: "not",
      filters: [{ field: "status", operator: "eq", value: "pending" }],
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(1);
    expect(result[0]?.status).toBe("done");
  });

  it("applies deeply nested AND/OR filter", () => {
    const tasks: Task[] = [
      makeTask({ status: "pending", priority: "critical", assignee: "alice" }),
      makeTask({ status: "pending", priority: "low", assignee: null }),
      makeTask({ status: "done", priority: "critical", assignee: "bob" }),
    ];
    // (status = 'pending' AND priority = 'critical') OR (assignee IS NULL)
    const filter: Filter = {
      operator: "or",
      filters: [
        {
          operator: "and",
          filters: [
            { field: "status", operator: "eq", value: "pending" },
            { field: "priority", operator: "eq", value: "critical" },
          ],
        },
        { field: "assignee", operator: "isNull", value: null },
      ],
    };
    const result = engine.applyFilter(tasks, filter);
    // Matches task 0 (pending+critical) and task 1 (null assignee)
    expect(result.length).toBe(2);
  });

  // -------------------------------------------------------------------------
  // Comparison operators
  // -------------------------------------------------------------------------

  it("applies gte filter on priority using weight ordering", () => {
    const tasks: Task[] = [
      makeTask({ priority: "low" }),
      makeTask({ priority: "medium" }),
      makeTask({ priority: "high" }),
      makeTask({ priority: "critical" }),
    ];
    const filter: Filter = {
      field: "priority",
      operator: "gte",
      value: "high",
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(2);
    expect(result.every((t) => t.priority === "high" || t.priority === "critical")).toBe(true);
  });

  it("applies lt filter on numeric estimatedHours", () => {
    const tasks: Task[] = [
      makeTask({ estimatedHours: 1 }),
      makeTask({ estimatedHours: 5 }),
      makeTask({ estimatedHours: 10 }),
    ];
    const filter: Filter = {
      field: "estimatedHours",
      operator: "lt",
      value: 5,
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(1);
    expect(result[0]?.estimatedHours).toBe(1);
  });

  it("applies gt filter on numeric actualHours", () => {
    const tasks: Task[] = [
      makeTask({ actualHours: 3 }),
      makeTask({ actualHours: 7 }),
      makeTask({ actualHours: 12 }),
    ];
    const filter: Filter = {
      field: "actualHours",
      operator: "gt",
      value: 5,
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(2);
    expect(result.every((t) => (t.actualHours ?? 0) > 5)).toBe(true);
  });

  it("applies lte filter", () => {
    const tasks: Task[] = [
      makeTask({ estimatedHours: 2 }),
      makeTask({ estimatedHours: 4 }),
      makeTask({ estimatedHours: 6 }),
    ];
    const filter: Filter = {
      field: "estimatedHours",
      operator: "lte",
      value: 4,
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(2);
  });

  // -------------------------------------------------------------------------
  // String operators
  // -------------------------------------------------------------------------

  it("applies contains filter on title", () => {
    const tasks: Task[] = [
      makeTask({ title: "Fix login bug" }),
      makeTask({ title: "Add logout feature" }),
      makeTask({ title: "Write tests" }),
    ];
    const filter: Filter = {
      field: "title",
      operator: "contains",
      value: "log",
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(2);
  });

  it("applies startsWith filter on title", () => {
    const tasks: Task[] = [
      makeTask({ title: "Fix login bug" }),
      makeTask({ title: "fix typo" }),
      makeTask({ title: "Add feature" }),
    ];
    const filter: Filter = {
      field: "title",
      operator: "startsWith",
      value: "fix",
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(2);
  });

  it("applies endsWith filter on title", () => {
    const tasks: Task[] = [
      makeTask({ title: "Fix bug" }),
      makeTask({ title: "Report bug" }),
      makeTask({ title: "Add feature" }),
    ];
    const filter: Filter = {
      field: "title",
      operator: "endsWith",
      value: "bug",
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(2);
  });

  // -------------------------------------------------------------------------
  // Null checks
  // -------------------------------------------------------------------------

  it("applies isNull filter on assignee", () => {
    const tasks: Task[] = [
      makeTask({ assignee: null }),
      makeTask({ assignee: "alice" }),
      makeTask({ assignee: null }),
    ];
    const filter: Filter = {
      field: "assignee",
      operator: "isNull",
      value: null,
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(2);
    expect(result.every((t) => t.assignee === null)).toBe(true);
  });

  it("applies isNotNull filter on dueDate", () => {
    const future = new Date("2030-01-01T00:00:00.000Z");
    const tasks: Task[] = [
      makeTask({ dueDate: null }),
      makeTask({ dueDate: future }),
      makeTask({ dueDate: future }),
    ];
    const filter: Filter = {
      field: "dueDate",
      operator: "isNotNull",
      value: null,
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(2);
  });

  // -------------------------------------------------------------------------
  // Membership
  // -------------------------------------------------------------------------

  it("applies in filter", () => {
    const tasks: Task[] = [
      makeTask({ status: "pending" }),
      makeTask({ status: "in_progress" }),
      makeTask({ status: "done" }),
      makeTask({ status: "cancelled" }),
    ];
    const filter: Filter = {
      field: "status",
      operator: "in",
      value: ["pending", "in_progress"],
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(2);
  });

  it("applies notIn filter", () => {
    const tasks: Task[] = [
      makeTask({ priority: "low" }),
      makeTask({ priority: "medium" }),
      makeTask({ priority: "high" }),
    ];
    const filter: Filter = {
      field: "priority",
      operator: "notIn",
      value: ["low", "medium"],
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(1);
    expect(result[0]?.priority).toBe("high");
  });

  it("applies contains filter on tags array", () => {
    const tasks: Task[] = [
      makeTask({ tags: ["urgent", "backend"] }),
      makeTask({ tags: ["frontend"] }),
      makeTask({ tags: ["urgent", "design"] }),
    ];
    const filter: Filter = {
      field: "tags",
      operator: "contains",
      value: "urgent",
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(2);
  });

  // -------------------------------------------------------------------------
  // Metadata sub-field access
  // -------------------------------------------------------------------------

  it("filters on metadata sub-field using dot notation", () => {
    const tasks: Task[] = [
      makeTask({ metadata: { sprint: 1 } }),
      makeTask({ metadata: { sprint: 2 } }),
      makeTask({ metadata: { sprint: 1 } }),
    ];
    const filter: Filter = {
      field: "metadata.sprint",
      operator: "eq",
      value: 1,
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(2);
  });

  // -------------------------------------------------------------------------
  // Date comparison
  // -------------------------------------------------------------------------

  it("filters tasks by dueDate greater than a reference date", () => {
    const jan = new Date("2024-01-15T00:00:00.000Z");
    const mar = new Date("2024-03-15T00:00:00.000Z");
    const jun = new Date("2024-06-15T00:00:00.000Z");
    const tasks: Task[] = [
      makeTask({ dueDate: jan }),
      makeTask({ dueDate: mar }),
      makeTask({ dueDate: jun }),
    ];
    const filter: Filter = {
      field: "dueDate",
      operator: "gt",
      value: new Date("2024-02-01T00:00:00.000Z"),
    };
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(2);
  });

  // -------------------------------------------------------------------------
  // Sorting
  // -------------------------------------------------------------------------

  it("sorts tasks by priority descending", () => {
    const tasks: Task[] = [
      makeTask({ priority: "low" }),
      makeTask({ priority: "critical" }),
      makeTask({ priority: "medium" }),
      makeTask({ priority: "high" }),
    ];
    const sort: SortSpec[] = [{ field: "priority", direction: "desc" }];
    const sorted = engine.applySort(tasks, sort);
    expect(sorted.map((t) => t.priority)).toEqual([
      "critical",
      "high",
      "medium",
      "low",
    ]);
  });

  it("sorts tasks by priority ascending", () => {
    const tasks: Task[] = [
      makeTask({ priority: "critical" }),
      makeTask({ priority: "low" }),
      makeTask({ priority: "high" }),
    ];
    const sort: SortSpec[] = [{ field: "priority", direction: "asc" }];
    const sorted = engine.applySort(tasks, sort);
    expect(sorted.map((t) => t.priority)).toEqual(["low", "high", "critical"]);
  });

  it("sorts tasks by estimatedHours ascending with nulls last", () => {
    const tasks: Task[] = [
      makeTask({ estimatedHours: 5 }),
      makeTask({ estimatedHours: null }),
      makeTask({ estimatedHours: 2 }),
    ];
    const sort: SortSpec[] = [
      { field: "estimatedHours", direction: "asc", nulls: "last" },
    ];
    const sorted = engine.applySort(tasks, sort);
    expect(sorted[0]?.estimatedHours).toBe(2);
    expect(sorted[1]?.estimatedHours).toBe(5);
    expect(sorted[2]?.estimatedHours).toBeNull();
  });

  it("sorts tasks by estimatedHours ascending with nulls first", () => {
    const tasks: Task[] = [
      makeTask({ estimatedHours: 5 }),
      makeTask({ estimatedHours: null }),
      makeTask({ estimatedHours: 2 }),
    ];
    const sort: SortSpec[] = [
      { field: "estimatedHours", direction: "asc", nulls: "first" },
    ];
    const sorted = engine.applySort(tasks, sort);
    expect(sorted[0]?.estimatedHours).toBeNull();
    expect(sorted[1]?.estimatedHours).toBe(2);
    expect(sorted[2]?.estimatedHours).toBe(5);
  });

  it("sorts tasks by title alphabetically", () => {
    const tasks: Task[] = [
      makeTask({ title: "Zebra" }),
      makeTask({ title: "Apple" }),
      makeTask({ title: "Mango" }),
    ];
    const sort: SortSpec[] = [{ field: "title", direction: "asc" }];
    const sorted = engine.applySort(tasks, sort);
    expect(sorted.map((t) => t.title)).toEqual(["Apple", "Mango", "Zebra"]);
  });

  it("applies multi-key sort: status asc then priority desc", () => {
    const tasks: Task[] = [
      makeTask({ status: "pending", priority: "low" }),
      makeTask({ status: "pending", priority: "high" }),
      makeTask({ status: "done", priority: "critical" }),
    ];
    const sort: SortSpec[] = [
      { field: "status", direction: "asc" },
      { field: "priority", direction: "desc" },
    ];
    const sorted = engine.applySort(tasks, sort);
    // done comes before pending alphabetically
    expect(sorted[0]?.status).toBe("done");
    // Among pending, high before low
    expect(sorted[1]?.priority).toBe("high");
    expect(sorted[2]?.priority).toBe("low");
  });

  it("applySort with no sort keys returns original order", () => {
    const tasks: Task[] = [
      makeTask({ title: "Z" }),
      makeTask({ title: "A" }),
    ];
    const sorted = engine.applySort(tasks, []);
    expect(sorted[0]?.title).toBe("Z");
    expect(sorted[1]?.title).toBe("A");
  });

  // -------------------------------------------------------------------------
  // applyQuery — full pipeline
  // -------------------------------------------------------------------------

  it("applyQuery applies filter, sort, and pagination", () => {
    const tasks: Task[] = [
      makeTask({ status: "pending", priority: "low" }),
      makeTask({ status: "pending", priority: "critical" }),
      makeTask({ status: "pending", priority: "high" }),
      makeTask({ status: "pending", priority: "medium" }),
      makeTask({ status: "done", priority: "critical" }),
    ];

    const result = engine.applyQuery(tasks, {
      filter: { field: "status", operator: "eq", value: "pending" },
      sort: [{ field: "priority", direction: "desc" }],
      limit: 2,
      offset: 0,
    });

    expect(result.total).toBe(4); // 4 pending tasks total
    expect(result.items.length).toBe(2);
    expect(result.hasMore).toBe(true);
    expect(result.items[0]?.priority).toBe("critical");
    expect(result.items[1]?.priority).toBe("high");
  });

  it("applyQuery with offset pages through results", () => {
    const tasks: Task[] = [];
    for (let i = 0; i < 10; i++) {
      tasks.push(makeTask({ estimatedHours: i }));
    }

    const page2 = engine.applyQuery(tasks, {
      sort: [{ field: "estimatedHours", direction: "asc" }],
      limit: 4,
      offset: 4,
    });

    expect(page2.offset).toBe(4);
    expect(page2.items.length).toBe(4);
    expect(page2.items[0]?.estimatedHours).toBe(4);
    expect(page2.items[3]?.estimatedHours).toBe(7);
    expect(page2.hasMore).toBe(true);
  });

  it("applyQuery with no options returns all tasks", () => {
    const tasks: Task[] = [makeTask(), makeTask(), makeTask()];
    const result = engine.applyQuery(tasks, {});
    expect(result.total).toBe(3);
    expect(result.items.length).toBe(3);
    expect(result.hasMore).toBe(false);
  });

  // -------------------------------------------------------------------------
  // FilterExpressionParser
  // -------------------------------------------------------------------------

  it("parses a simple equality expression", () => {
    const parser = new FilterExpressionParser();
    const filter = parser.parse("status = 'pending'");
    expect(filter).toMatchObject({
      field: "status",
      operator: "eq",
      value: "pending",
    });
  });

  it("parses an AND expression", () => {
    const parser = new FilterExpressionParser();
    const filter = parser.parse("status = 'pending' AND priority = 'high'");
    expect(filter).toMatchObject({
      operator: "and",
      filters: [
        { field: "status", operator: "eq", value: "pending" },
        { field: "priority", operator: "eq", value: "high" },
      ],
    });
  });

  it("parses an OR expression", () => {
    const parser = new FilterExpressionParser();
    const filter = parser.parse("status = 'done' OR status = 'cancelled'");
    expect(filter).toMatchObject({ operator: "or" });
  });

  it("parses a NOT expression", () => {
    const parser = new FilterExpressionParser();
    const filter = parser.parse("NOT status = 'done'");
    expect(filter).toMatchObject({ operator: "not" });
  });

  it("parses IS NULL expression", () => {
    const parser = new FilterExpressionParser();
    const filter = parser.parse("assignee IS NULL");
    expect(filter).toMatchObject({
      field: "assignee",
      operator: "isNull",
    });
  });

  it("parses IS NOT NULL expression", () => {
    const parser = new FilterExpressionParser();
    const filter = parser.parse("dueDate IS NOT NULL");
    expect(filter).toMatchObject({
      field: "dueDate",
      operator: "isNotNull",
    });
  });

  it("parses IN expression", () => {
    const parser = new FilterExpressionParser();
    const filter = parser.parse("status IN ('pending', 'in_progress')");
    expect(filter).toMatchObject({
      field: "status",
      operator: "in",
      value: ["pending", "in_progress"],
    });
  });

  it("parses NOT IN expression", () => {
    const parser = new FilterExpressionParser();
    const filter = parser.parse("priority NOT IN ('low', 'medium')");
    expect(filter).toMatchObject({
      field: "priority",
      operator: "notIn",
      value: ["low", "medium"],
    });
  });

  it("parses CONTAINS expression", () => {
    const parser = new FilterExpressionParser();
    const filter = parser.parse("title CONTAINS 'bug'");
    expect(filter).toMatchObject({
      field: "title",
      operator: "contains",
      value: "bug",
    });
  });

  it("parses STARTSWITH expression", () => {
    const parser = new FilterExpressionParser();
    const filter = parser.parse("title STARTSWITH 'Fix'");
    expect(filter).toMatchObject({
      field: "title",
      operator: "startsWith",
      value: "Fix",
    });
  });

  it("parses ENDSWITH expression", () => {
    const parser = new FilterExpressionParser();
    const filter = parser.parse("title ENDSWITH 'bug'");
    expect(filter).toMatchObject({
      field: "title",
      operator: "endsWith",
      value: "bug",
    });
  });

  it("parses numeric value", () => {
    const parser = new FilterExpressionParser();
    const filter = parser.parse("estimatedHours >= 4");
    expect(filter).toMatchObject({
      field: "estimatedHours",
      operator: "gte",
      value: 4,
    });
  });

  it("parses boolean value TRUE", () => {
    const parser = new FilterExpressionParser();
    const filter = parser.parse("metadata.active = TRUE");
    expect(filter).toMatchObject({
      field: "metadata.active",
      operator: "eq",
      value: true,
    });
  });

  it("parses complex nested expression with parentheses", () => {
    const parser = new FilterExpressionParser();
    const filter = parser.parse(
      "(status = 'pending' AND priority = 'critical') OR assignee IS NULL"
    );
    expect(filter).toMatchObject({ operator: "or" });
  });

  it("parsed filter expression evaluates correctly against tasks", () => {
    const parser = new FilterExpressionParser();
    const filter = parser.parse("status = 'pending' AND priority = 'high'");

    const tasks: Task[] = [
      makeTask({ status: "pending", priority: "high" }),
      makeTask({ status: "pending", priority: "low" }),
      makeTask({ status: "done", priority: "high" }),
    ];
    const result = engine.applyFilter(tasks, filter);
    expect(result.length).toBe(1);
    expect(result[0]?.priority).toBe("high");
  });

  it("throws QueryError for unterminated string literal", () => {
    const parser = new FilterExpressionParser();
    expect(() => parser.parse("title = 'unterminated")).toThrow();
  });

  it("throws QueryError for unexpected token", () => {
    const parser = new FilterExpressionParser();
    expect(() => parser.parse("=")).toThrow();
  });

  // -------------------------------------------------------------------------
  // serializeFilter
  // -------------------------------------------------------------------------

  it("serializeFilter produces a parseable round-trip", () => {
    const original: Filter = {
      operator: "and",
      filters: [
        { field: "status", operator: "eq", value: "pending" },
        { field: "priority", operator: "gte", value: "high" },
      ],
    };
    const serialized = serializeFilter(original);
    expect(serialized).toContain("AND");
    expect(serialized).toContain("status");
    expect(serialized).toContain("priority");
  });

  it("serializeFilter handles isNull", () => {
    const filter: Filter = {
      field: "assignee",
      operator: "isNull",
      value: null,
    };
    const serialized = serializeFilter(filter);
    expect(serialized).toBe("assignee IS NULL");
  });

  it("serializeFilter handles isNotNull", () => {
    const filter: Filter = {
      field: "dueDate",
      operator: "isNotNull",
      value: null,
    };
    const serialized = serializeFilter(filter);
    expect(serialized).toBe("dueDate IS NOT NULL");
  });

  it("serializeFilter handles in operator", () => {
    const filter: Filter = {
      field: "status",
      operator: "in",
      value: ["pending", "in_progress"],
    };
    const serialized = serializeFilter(filter);
    expect(serialized).toContain("IN");
    expect(serialized).toContain("pending");
  });
});
