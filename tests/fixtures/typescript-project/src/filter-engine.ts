/**
 * @fileoverview Filter / sort expression engine for UCIL task queries.
 *
 * This module provides two top-level classes:
 *
 * - {@link FilterEngine}  — evaluates a {@link Filter} tree against a task set,
 *   applies multi-key sorting, and combines them into paginated
 *   {@link QueryResult} values.
 *
 * - {@link FilterExpressionParser} — compiles a human-readable expression
 *   string (e.g. `"status = 'pending' AND priority >= 'high'"`) into a
 *   {@link Filter} tree that {@link FilterEngine} can evaluate.
 *
 * No external runtime dependencies are used; the implementation is
 * self-contained pure TypeScript.
 */

import {
  type Task,
  type Filter,
  type FieldFilter,
  type LogicalFilter,
  type FilterOperator,
  type SortSpec,
  type Query,
  type QueryResult,
  type Token,
  type TokenKind,
  isFieldFilter,
  isLogicalFilter,
  PRIORITY_WEIGHT,
  DEFAULT_QUERY_LIMIT,
  MAX_QUERY_LIMIT,
  QueryError,
} from "./types.js";

// ---------------------------------------------------------------------------
// FilterEngine
// ---------------------------------------------------------------------------

/**
 * Stateless engine that evaluates filter trees and sort specifications against
 * arrays of {@link Task} objects.
 *
 * @example
 * ```typescript
 * const engine = new FilterEngine();
 * const pending = engine.applyFilter(allTasks, {
 *   field: "status", operator: "eq", value: "pending"
 * });
 * ```
 */
export class FilterEngine {
  // -------------------------------------------------------------------------
  // Public API
  // -------------------------------------------------------------------------

  /**
   * Returns the subset of `tasks` that satisfy `filter`.
   *
   * @param tasks  — Source task array (not mutated).
   * @param filter — Filter tree to evaluate.
   */
  applyFilter(tasks: Task[], filter: Filter): Task[] {
    return tasks.filter((t) => this.evaluateFilter(t, filter));
  }

  /**
   * Returns a new array containing `tasks` sorted according to `sort`.
   *
   * Sort keys are applied left-to-right; ties are broken by the next key.
   * When `sort` is empty the original order is preserved.
   *
   * @param tasks — Source task array (not mutated).
   * @param sort  — Ordered list of sort specifications.
   */
  applySort(tasks: Task[], sort: SortSpec[]): Task[] {
    if (sort.length === 0) return [...tasks];
    return [...tasks].sort((a, b) => {
      for (const spec of sort) {
        const cmp = this.sortComparator(a, b, spec);
        if (cmp !== 0) return cmp;
      }
      return 0;
    });
  }

  /**
   * Applies `query.filter`, `query.sort`, and pagination (`limit`/`offset`)
   * to `tasks` and returns a paginated {@link QueryResult}.
   *
   * If `query.fields` is specified, only those fields are projected into each
   * result item.
   *
   * @param tasks — Full task array (not mutated).
   * @param query — The query descriptor.
   */
  applyQuery(tasks: Task[], query: Query): QueryResult {
    // Step 1: filter
    let filtered: Task[] =
      query.filter !== undefined
        ? this.applyFilter(tasks, query.filter)
        : [...tasks];

    // Step 2: sort
    if (query.sort !== undefined && query.sort.length > 0) {
      filtered = this.applySort(filtered, query.sort);
    }

    // Step 3: pagination
    const total = filtered.length;
    const offset = query.offset ?? 0;
    const limit = Math.min(
      query.limit ?? DEFAULT_QUERY_LIMIT,
      MAX_QUERY_LIMIT
    );
    const page = filtered.slice(offset, offset + limit);

    // Step 4: field projection
    const items: Task[] =
      query.fields !== undefined && query.fields.length > 0
        ? (page.map((t) => projectTask(t, query.fields!)) as Task[])
        : page;

    return {
      items,
      total,
      hasMore: offset + limit < total,
      offset,
      limit,
    };
  }

  // -------------------------------------------------------------------------
  // Private: filter evaluation
  // -------------------------------------------------------------------------

  /**
   * Dispatches to the appropriate evaluator based on whether `filter` is a
   * leaf or branch node.
   */
  private evaluateFilter(task: Task, filter: Filter): boolean {
    if (isFieldFilter(filter)) {
      return this.evaluateFieldFilter(task, filter);
    }
    if (isLogicalFilter(filter)) {
      return this.evaluateLogicalFilter(task, filter);
    }
    throw new QueryError(
      JSON.stringify(filter),
      "Unknown filter node type — expected FieldFilter or LogicalFilter."
    );
  }

  /**
   * Evaluates a leaf {@link FieldFilter} against `task`.
   *
   * @throws {@link QueryError} for unknown operators.
   */
  private evaluateFieldFilter(task: Task, filter: FieldFilter): boolean {
    const fieldValue = this.getFieldValue(task, filter.field as string);
    return this.compareValues(fieldValue, filter.value, filter.operator);
  }

  /**
   * Evaluates a branch {@link LogicalFilter} by recursively evaluating each
   * child filter.
   *
   * - `and`: every child must be true
   * - `or`:  at least one child must be true
   * - `not`: the single child must be false
   *
   * @throws {@link QueryError} for `not` with a count other than 1.
   */
  private evaluateLogicalFilter(task: Task, filter: LogicalFilter): boolean {
    switch (filter.operator) {
      case "and":
        return filter.filters.every((f) => this.evaluateFilter(task, f));
      case "or":
        return filter.filters.some((f) => this.evaluateFilter(task, f));
      case "not": {
        if (filter.filters.length !== 1) {
          throw new QueryError(
            JSON.stringify(filter),
            `LogicalFilter "not" must have exactly one child; got ${filter.filters.length}.`
          );
        }
        const child = filter.filters[0];
        if (child === undefined) {
          throw new QueryError(
            JSON.stringify(filter),
            "LogicalFilter \"not\" child is undefined."
          );
        }
        return !this.evaluateFilter(task, child);
      }
      default: {
        // TypeScript exhaustiveness — operator is a union type
        const exhaustive: never = filter.operator;
        throw new QueryError(
          String(exhaustive),
          `Unknown logical operator: ${String(exhaustive)}`
        );
      }
    }
  }

  // -------------------------------------------------------------------------
  // Private: field access
  // -------------------------------------------------------------------------

  /**
   * Retrieves the value of `field` from `task`.
   *
   * Supports top-level `keyof Task` fields and dot-notation paths into
   * `metadata` (e.g. `"metadata.project"`).
   *
   * @returns The field value, or `undefined` when the path does not exist.
   */
  private getFieldValue(task: Task, field: string): unknown {
    if (field.startsWith("metadata.")) {
      const subKey = field.slice("metadata.".length);
      return task.metadata[subKey];
    }
    // Direct field access; the cast is safe because we validated `field`
    // against `keyof Task` at the point of filter construction.
    return (task as Record<string, unknown>)[field];
  }

  // -------------------------------------------------------------------------
  // Private: value comparison
  // -------------------------------------------------------------------------

  /**
   * Applies `operator` to `fieldValue` and `filterValue`.
   *
   * The method handles type coercion for date comparisons and normalises
   * strings to lower-case for `contains` / `startsWith` / `endsWith`.
   *
   * @throws {@link QueryError} for unsupported type/operator combinations.
   */
  private compareValues(
    fieldValue: unknown,
    filterValue: unknown,
    operator: FilterOperator
  ): boolean {
    // Null checks do not need the filterValue.
    if (operator === "isNull") return fieldValue === null || fieldValue === undefined;
    if (operator === "isNotNull") return fieldValue !== null && fieldValue !== undefined;

    // Membership checks
    if (operator === "in") {
      if (!Array.isArray(filterValue)) {
        throw new QueryError(
          String(filterValue),
          'Operator "in" requires an array value.'
        );
      }
      return filterValue.some((v) => this.looseEquals(fieldValue, v));
    }
    if (operator === "notIn") {
      if (!Array.isArray(filterValue)) {
        throw new QueryError(
          String(filterValue),
          'Operator "notIn" requires an array value.'
        );
      }
      return !filterValue.some((v) => this.looseEquals(fieldValue, v));
    }

    // String operations
    if (operator === "contains") {
      return this.stringContains(fieldValue, filterValue);
    }
    if (operator === "startsWith") {
      if (typeof fieldValue !== "string" || typeof filterValue !== "string") {
        return false;
      }
      return fieldValue.toLowerCase().startsWith(filterValue.toLowerCase());
    }
    if (operator === "endsWith") {
      if (typeof fieldValue !== "string" || typeof filterValue !== "string") {
        return false;
      }
      return fieldValue.toLowerCase().endsWith(filterValue.toLowerCase());
    }

    // Equality
    if (operator === "eq") return this.looseEquals(fieldValue, filterValue);
    if (operator === "neq") return !this.looseEquals(fieldValue, filterValue);

    // Ordered comparison
    const cmp = this.orderedCompare(fieldValue, filterValue);
    if (cmp === null) return false;
    switch (operator) {
      case "lt": return cmp < 0;
      case "lte": return cmp <= 0;
      case "gt": return cmp > 0;
      case "gte": return cmp >= 0;
      default: {
        const exhaustive: never = operator;
        throw new QueryError(
          String(exhaustive),
          `Unknown operator: ${String(exhaustive)}`
        );
      }
    }
  }

  /**
   * Loose equality that handles Date ↔ string / number comparisons and
   * priority string ordering.
   */
  private looseEquals(a: unknown, b: unknown): boolean {
    if (a === b) return true;
    if (a instanceof Date && typeof b === "string") {
      return a.toISOString() === b || a.getTime() === new Date(b).getTime();
    }
    if (typeof a === "string" && b instanceof Date) {
      return new Date(a).getTime() === b.getTime();
    }
    return false;
  }

  /**
   * Returns a comparison result for ordered types: negative, zero, or positive.
   *
   * Returns `null` when the values are not orderable.
   */
  private orderedCompare(a: unknown, b: unknown): number | null {
    if (typeof a === "number" && typeof b === "number") return a - b;
    if (a instanceof Date && b instanceof Date) return a.getTime() - b.getTime();
    if (a instanceof Date && typeof b === "string") {
      return a.getTime() - new Date(b).getTime();
    }
    if (typeof a === "string" && b instanceof Date) {
      return new Date(a).getTime() - b.getTime();
    }
    if (typeof a === "string" && typeof b === "string") {
      // Special case: priority strings should compare by weight
      const wa = PRIORITY_WEIGHT[a as keyof typeof PRIORITY_WEIGHT];
      const wb = PRIORITY_WEIGHT[b as keyof typeof PRIORITY_WEIGHT];
      if (wa !== undefined && wb !== undefined) return wa - wb;
      return a.localeCompare(b);
    }
    return null;
  }

  /**
   * Handles `contains` for both strings and arrays.
   */
  private stringContains(fieldValue: unknown, filterValue: unknown): boolean {
    if (Array.isArray(fieldValue)) {
      return fieldValue.some((v) => this.looseEquals(v, filterValue));
    }
    if (typeof fieldValue !== "string" || typeof filterValue !== "string") {
      return false;
    }
    return fieldValue.toLowerCase().includes(filterValue.toLowerCase());
  }

  // -------------------------------------------------------------------------
  // Private: sort comparator
  // -------------------------------------------------------------------------

  /**
   * Returns a sort comparison result for two tasks on a single sort key.
   *
   * Null values are placed according to `spec.nulls` (default: `"last"`).
   */
  private sortComparator(a: Task, b: Task, spec: SortSpec): number {
    const va = this.getFieldValue(a, spec.field as string);
    const vb = this.getFieldValue(b, spec.field as string);

    const aIsNull = va === null || va === undefined;
    const bIsNull = vb === null || vb === undefined;

    const nullsFirst = spec.nulls === "first";

    if (aIsNull && bIsNull) return 0;
    if (aIsNull) return nullsFirst ? -1 : 1;
    if (bIsNull) return nullsFirst ? 1 : -1;

    const cmp = this.orderedCompare(va, vb) ?? String(va).localeCompare(String(vb));
    return spec.direction === "desc" ? -cmp : cmp;
  }
}

// ---------------------------------------------------------------------------
// Field projection helper (module-local)
// ---------------------------------------------------------------------------

/**
 * Projects a task onto only the requested fields.
 * Returns a Partial<Task> cast as Task for convenience in the query result.
 */
function projectTask(task: Task, fields: Array<keyof Task>): Partial<Task> {
  const out: Partial<Task> = {};
  for (const f of fields) {
    (out as Record<string, unknown>)[f] = task[f];
  }
  return out;
}

// ---------------------------------------------------------------------------
// FilterExpressionParser — lexer
// ---------------------------------------------------------------------------

/** Maps keyword strings to their canonical token kind. */
const KEYWORDS: Record<string, TokenKind> = {
  AND: "AND",
  OR: "OR",
  NOT: "NOT",
  IS: "IS",
  IN: "IN",
  NULL: "NULL",
  TRUE: "BOOLEAN",
  FALSE: "BOOLEAN",
  CONTAINS: "CONTAINS",
  STARTSWITH: "STARTS_WITH",
  ENDSWITH: "ENDS_WITH",
};

/**
 * Converts a raw expression string into a flat list of tokens.
 *
 * @param source — The expression to tokenise.
 * @returns An array of tokens including a trailing `EOF`.
 * @throws {@link QueryError} for unrecognised characters or unterminated strings.
 */
function tokenize(source: string): Token[] {
  const tokens: Token[] = [];
  let i = 0;

  while (i < source.length) {
    // Skip whitespace
    if (/\s/.test(source[i] ?? "")) {
      i++;
      continue;
    }

    // Single-char punctuation
    if (source[i] === "(") { tokens.push({ kind: "LPAREN", value: "(", offset: i }); i++; continue; }
    if (source[i] === ")") { tokens.push({ kind: "RPAREN", value: ")", offset: i }); i++; continue; }
    if (source[i] === ",") { tokens.push({ kind: "COMMA", value: ",", offset: i }); i++; continue; }

    // Two-char operators
    if (source.slice(i, i + 2) === "!=") { tokens.push({ kind: "NEQ", value: "!=", offset: i }); i += 2; continue; }
    if (source.slice(i, i + 2) === "<>") { tokens.push({ kind: "NEQ", value: "<>", offset: i }); i += 2; continue; }
    if (source.slice(i, i + 2) === "<=") { tokens.push({ kind: "LTE", value: "<=", offset: i }); i += 2; continue; }
    if (source.slice(i, i + 2) === ">=") { tokens.push({ kind: "GTE", value: ">=", offset: i }); i += 2; continue; }

    // One-char operators
    if (source[i] === "=") { tokens.push({ kind: "EQ", value: "=", offset: i }); i++; continue; }
    if (source[i] === "<") { tokens.push({ kind: "LT", value: "<", offset: i }); i++; continue; }
    if (source[i] === ">") { tokens.push({ kind: "GT", value: ">", offset: i }); i++; continue; }

    // String literals (single or double quoted)
    if (source[i] === "'" || source[i] === '"') {
      const quote = source[i]!;
      const start = i;
      i++;
      let str = "";
      while (i < source.length && source[i] !== quote) {
        if (source[i] === "\\" && i + 1 < source.length) {
          i++;
          str += source[i];
        } else {
          str += source[i];
        }
        i++;
      }
      if (i >= source.length) {
        throw new QueryError(source, `Unterminated string literal at offset ${start}.`);
      }
      i++; // consume closing quote
      tokens.push({ kind: "STRING", value: str, offset: start });
      continue;
    }

    // Numbers
    if (/[0-9]/.test(source[i] ?? "")) {
      const start = i;
      let num = "";
      while (i < source.length && /[0-9.]/.test(source[i] ?? "")) {
        num += source[i];
        i++;
      }
      tokens.push({ kind: "NUMBER", value: num, offset: start });
      continue;
    }

    // Identifiers and keywords
    if (/[a-zA-Z_]/.test(source[i] ?? "")) {
      const start = i;
      let ident = "";
      while (i < source.length && /[a-zA-Z0-9_.:]/.test(source[i] ?? "")) {
        ident += source[i];
        i++;
      }
      const upper = ident.toUpperCase();
      const kind: TokenKind = KEYWORDS[upper] ?? "IDENT";
      tokens.push({ kind, value: ident, offset: start });
      continue;
    }

    throw new QueryError(
      source,
      `Unexpected character "${source[i] ?? ""}" at offset ${i}.`
    );
  }

  tokens.push({ kind: "EOF", value: "", offset: i });
  return tokens;
}

// ---------------------------------------------------------------------------
// FilterExpressionParser — recursive descent parser
// ---------------------------------------------------------------------------

/**
 * Parses a filter expression string into a {@link Filter} tree.
 *
 * Grammar (EBNF-style):
 * ```
 * expr        ::= or_expr
 * or_expr     ::= and_expr  ( "OR"  and_expr  )*
 * and_expr    ::= not_expr  ( "AND" not_expr  )*
 * not_expr    ::= "NOT" not_expr | primary
 * primary     ::= "(" expr ")" | field_filter
 * field_filter::= IDENT operator value_expr
 *               | IDENT "IS" "NULL"
 *               | IDENT "IS" "NOT" "NULL"
 *               | IDENT "IN" "(" value_list ")"
 *               | IDENT "NOT" "IN" "(" value_list ")"
 *               | IDENT "CONTAINS" value_expr
 *               | IDENT "STARTSWITH" value_expr
 *               | IDENT "ENDSWITH" value_expr
 * operator    ::= "=" | "!=" | "<>" | "<" | "<=" | ">" | ">="
 * value_expr  ::= STRING | NUMBER | BOOLEAN | NULL | IDENT
 * value_list  ::= value_expr ( "," value_expr )*
 * ```
 *
 * @example
 * ```typescript
 * const parser = new FilterExpressionParser();
 * const filter = parser.parse("status = 'pending' AND priority >= 'high'");
 * ```
 */
export class FilterExpressionParser {
  private tokens: Token[] = [];
  private pos: number = 0;

  /**
   * Parses `expression` into a {@link Filter} tree.
   *
   * @param expression — The filter expression string.
   * @returns The parsed filter.
   * @throws {@link QueryError} on syntax errors.
   */
  parse(expression: string): Filter {
    this.tokens = tokenize(expression);
    this.pos = 0;
    const filter = this.parseOr();
    this.expect("EOF");
    return filter;
  }

  // -------------------------------------------------------------------------
  // Parser: recursive descent
  // -------------------------------------------------------------------------

  /** or_expr ::= and_expr ( "OR" and_expr )* */
  private parseOr(): Filter {
    let left = this.parseAnd();
    while (this.peek().kind === "OR") {
      this.consume();
      const right = this.parseAnd();
      left = { operator: "or", filters: [left, right] } satisfies LogicalFilter;
    }
    return left;
  }

  /** and_expr ::= not_expr ( "AND" not_expr )* */
  private parseAnd(): Filter {
    let left = this.parseNot();
    while (this.peek().kind === "AND") {
      this.consume();
      const right = this.parseNot();
      left = { operator: "and", filters: [left, right] } satisfies LogicalFilter;
    }
    return left;
  }

  /** not_expr ::= "NOT" not_expr | primary */
  private parseNot(): Filter {
    if (this.peek().kind === "NOT") {
      this.consume();
      const child = this.parseNot();
      return { operator: "not", filters: [child] } satisfies LogicalFilter;
    }
    return this.parsePrimary();
  }

  /** primary ::= "(" expr ")" | field_filter */
  private parsePrimary(): Filter {
    if (this.peek().kind === "LPAREN") {
      this.consume(); // "("
      const inner = this.parseOr();
      this.expect("RPAREN");
      return inner;
    }
    return this.parseFieldFilter();
  }

  /**
   * Parses a leaf field filter of one of these forms:
   *
   * ```
   * IDENT = value
   * IDENT IS NULL
   * IDENT IS NOT NULL
   * IDENT IN (v1, v2, ...)
   * IDENT NOT IN (v1, v2, ...)
   * IDENT CONTAINS value
   * IDENT STARTSWITH value
   * IDENT ENDSWITH value
   * ```
   */
  private parseFieldFilter(): FieldFilter {
    const fieldToken = this.expect("IDENT");
    const field = fieldToken.value;
    const next = this.peek();

    // IS NULL / IS NOT NULL
    if (next.kind === "IS") {
      this.consume();
      if (this.peek().kind === "NOT") {
        this.consume();
        this.expect("NULL");
        return { field, operator: "isNotNull", value: null };
      }
      this.expect("NULL");
      return { field, operator: "isNull", value: null };
    }

    // IN (...)
    if (next.kind === "IN") {
      this.consume();
      const values = this.parseValueList();
      return { field, operator: "in", value: values };
    }

    // NOT IN (...)
    if (next.kind === "NOT") {
      this.consume();
      if (this.peek().kind === "IN") {
        this.consume();
        const values = this.parseValueList();
        return { field, operator: "notIn", value: values };
      }
      throw new QueryError(
        field,
        `Expected "IN" after "NOT" for field "${field}".`
      );
    }

    // CONTAINS value
    if (next.kind === "CONTAINS") {
      this.consume();
      const val = this.parseValueExpr();
      return { field, operator: "contains", value: val };
    }

    // STARTSWITH value
    if (next.kind === "STARTS_WITH") {
      this.consume();
      const val = this.parseValueExpr();
      return { field, operator: "startsWith", value: val };
    }

    // ENDSWITH value
    if (next.kind === "ENDS_WITH") {
      this.consume();
      const val = this.parseValueExpr();
      return { field, operator: "endsWith", value: val };
    }

    // Standard comparison operators
    const operator = this.parseComparisonOperator();
    const value = this.parseValueExpr();
    return { field, operator, value };
  }

  /** Parses a comparison operator token and returns the matching {@link FilterOperator}. */
  private parseComparisonOperator(): FilterOperator {
    const t = this.consume();
    switch (t.kind) {
      case "EQ": return "eq";
      case "NEQ": return "neq";
      case "LT": return "lt";
      case "LTE": return "lte";
      case "GT": return "gt";
      case "GTE": return "gte";
      default:
        throw new QueryError(
          t.value,
          `Expected a comparison operator at offset ${t.offset}; got "${t.value}".`
        );
    }
  }

  /** Parses a single value literal (string, number, boolean, null, or identifier). */
  private parseValueExpr(): unknown {
    const t = this.consume();
    switch (t.kind) {
      case "STRING": return t.value;
      case "NUMBER": return Number(t.value);
      case "BOOLEAN": return t.value.toLowerCase() === "true";
      case "NULL": return null;
      case "IDENT": return t.value; // treat bare identifier as string value
      default:
        throw new QueryError(
          t.value,
          `Expected a value at offset ${t.offset}; got "${t.value}".`
        );
    }
  }

  /** Parses `"(" value_expr ( "," value_expr )* ")"` and returns an array of values. */
  private parseValueList(): unknown[] {
    this.expect("LPAREN");
    const values: unknown[] = [];
    values.push(this.parseValueExpr());
    while (this.peek().kind === "COMMA") {
      this.consume();
      values.push(this.parseValueExpr());
    }
    this.expect("RPAREN");
    return values;
  }

  // -------------------------------------------------------------------------
  // Token stream helpers
  // -------------------------------------------------------------------------

  /** Returns the current token without consuming it. */
  private peek(): Token {
    const t = this.tokens[this.pos];
    if (t === undefined) {
      return { kind: "EOF", value: "", offset: -1 };
    }
    return t;
  }

  /**
   * Consumes and returns the current token.
   *
   * @throws {@link QueryError} when the stream is exhausted.
   */
  private consume(): Token {
    const t = this.tokens[this.pos];
    if (t === undefined) {
      throw new QueryError("(EOF)", "Unexpected end of expression.");
    }
    this.pos++;
    return t;
  }

  /**
   * Asserts the current token has `kind`, consumes it, and returns it.
   *
   * @throws {@link QueryError} on mismatch.
   */
  private expect(kind: TokenKind): Token {
    const t = this.peek();
    if (t.kind !== kind) {
      throw new QueryError(
        t.value,
        `Expected token "${kind}" at offset ${t.offset}; got "${t.kind}" ("${t.value}").`
      );
    }
    return this.consume();
  }
}

// ---------------------------------------------------------------------------
// Convenience functions (module-level)
// ---------------------------------------------------------------------------

/**
 * Shorthand: create a new {@link FilterEngine} and apply a filter in one call.
 *
 * @param tasks  — Source array.
 * @param filter — Filter to apply.
 */
export function filterTasks(tasks: Task[], filter: Filter): Task[] {
  return new FilterEngine().applyFilter(tasks, filter);
}

/**
 * Shorthand: create a new {@link FilterEngine} and sort in one call.
 *
 * @param tasks — Source array.
 * @param sort  — Sort specification.
 */
export function sortTasks(tasks: Task[], sort: SortSpec[]): Task[] {
  return new FilterEngine().applySort(tasks, sort);
}

/**
 * Shorthand: create a new {@link FilterEngine} and run a full query in one call.
 *
 * @param tasks — Source array.
 * @param query — Query descriptor.
 */
export function queryTasks(tasks: Task[], query: Query): QueryResult {
  return new FilterEngine().applyQuery(tasks, query);
}

/**
 * Shorthand: parse an expression string into a {@link Filter}.
 *
 * @param expression — The expression to parse.
 */
export function parseFilter(expression: string): Filter {
  return new FilterExpressionParser().parse(expression);
}

// ---------------------------------------------------------------------------
// Built-in filter presets
// ---------------------------------------------------------------------------

/** A filter that matches all tasks with `status = "pending"`. */
export const FILTER_PENDING: FieldFilter = {
  field: "status",
  operator: "eq",
  value: "pending",
};

/** A filter that matches all tasks with `status = "done"`. */
export const FILTER_DONE: FieldFilter = {
  field: "status",
  operator: "eq",
  value: "done",
};

/** A filter that matches all tasks with `priority = "critical"`. */
export const FILTER_CRITICAL: FieldFilter = {
  field: "priority",
  operator: "eq",
  value: "critical",
};

/** A filter that matches unassigned tasks (`assignee IS NULL`). */
export const FILTER_UNASSIGNED: FieldFilter = {
  field: "assignee",
  operator: "isNull",
  value: null,
};

// ---------------------------------------------------------------------------
// Sort preset helpers
// ---------------------------------------------------------------------------

/**
 * Returns a sort specification that orders tasks by priority descending (most
 * urgent first) with nulls last.
 */
export function sortByPriorityDesc(): SortSpec {
  return { field: "priority", direction: "desc", nulls: "last" };
}

/**
 * Returns a sort specification that orders tasks by `dueDate` ascending
 * (earliest deadline first) with nulls last.
 */
export function sortByDueDateAsc(): SortSpec {
  return { field: "dueDate", direction: "asc", nulls: "last" };
}

/**
 * Returns a sort specification that orders tasks by `createdAt` descending
 * (newest first).
 */
export function sortByCreatedAtDesc(): SortSpec {
  return { field: "createdAt", direction: "desc" };
}

// ---------------------------------------------------------------------------
// Expression serialiser (Filter → string)
// ---------------------------------------------------------------------------

/**
 * Converts a {@link Filter} tree back into a human-readable expression string.
 *
 * This is the inverse of {@link FilterExpressionParser.parse} and is useful
 * for logging, debugging, and displaying saved queries to end users.
 *
 * @param filter — The filter to serialise.
 * @returns A SQL-like expression string.
 */
export function serializeFilter(filter: Filter): string {
  if (isFieldFilter(filter)) {
    return serializeFieldFilter(filter);
  }
  if (isLogicalFilter(filter)) {
    return serializeLogicalFilter(filter);
  }
  return "(unknown)";
}

function serializeFieldFilter(filter: FieldFilter): string {
  const { field, operator, value } = filter;
  switch (operator) {
    case "isNull": return `${field} IS NULL`;
    case "isNotNull": return `${field} IS NOT NULL`;
    case "in": return `${field} IN (${(value as unknown[]).map(serializeValue).join(", ")})`;
    case "notIn": return `${field} NOT IN (${(value as unknown[]).map(serializeValue).join(", ")})`;
    case "contains": return `${field} CONTAINS ${serializeValue(value)}`;
    case "startsWith": return `${field} STARTSWITH ${serializeValue(value)}`;
    case "endsWith": return `${field} ENDSWITH ${serializeValue(value)}`;
    case "eq": return `${field} = ${serializeValue(value)}`;
    case "neq": return `${field} != ${serializeValue(value)}`;
    case "lt": return `${field} < ${serializeValue(value)}`;
    case "lte": return `${field} <= ${serializeValue(value)}`;
    case "gt": return `${field} > ${serializeValue(value)}`;
    case "gte": return `${field} >= ${serializeValue(value)}`;
    default: {
      const exhaustive: never = operator;
      return `${field} ${String(exhaustive)} ${serializeValue(value)}`;
    }
  }
}

function serializeLogicalFilter(filter: LogicalFilter): string {
  if (filter.operator === "not") {
    const child = filter.filters[0];
    return `NOT (${child !== undefined ? serializeFilter(child) : ""})`;
  }
  const op = filter.operator.toUpperCase();
  return filter.filters
    .map((f) => `(${serializeFilter(f)})`)
    .join(` ${op} `);
}

function serializeValue(value: unknown): string {
  if (value === null) return "NULL";
  if (typeof value === "string") return `'${value.replace(/'/g, "\\'")}'`;
  if (typeof value === "boolean") return value ? "TRUE" : "FALSE";
  if (typeof value === "number") return String(value);
  if (value instanceof Date) return `'${value.toISOString()}'`;
  return JSON.stringify(value);
}
