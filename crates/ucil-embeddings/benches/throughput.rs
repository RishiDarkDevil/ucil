//! `CodeRankEmbed` throughput benchmark — `P2-W8-F06` / `WO-0061`.
//!
//! Master-plan §4.2 line 303 (verbatim): "`CodeRankEmbed` (137M params,
//! MIT license, 8K context) ... CPU-friendly, 50-150 embeddings/sec,
//! ~137MB with Int8 quantization".  Master-plan §18 Phase 2 Week 8
//! line 1789 (verbatim): "Benchmark: embedding throughput, query
//! latency, recall@10".
//!
//! This file lands the throughput half (the third of the three Week 8
//! benchmarks).  Query-latency is `P2-W8-F07`; recall@10 is
//! out-of-scope for this WO (needs a labelled query/answer corpus).
//!
//! # Bench shape
//!
//! - Loads the production [`CodeRankEmbed`] model from
//!   `ml/models/coderankembed/` (operator-installed via
//!   `scripts/devtools/install-coderankembed.sh` — the bench
//!   `panic!`s with an actionable message if the artefacts are
//!   absent; the bench script's pre-flight runs the installer
//!   idempotently).
//! - Iterates over [`SNIPPETS`] — 100 baked-in real code snippets
//!   (Rust + Python + TypeScript + plain text) — and calls
//!   [`CodeRankEmbed::embed`] on each, per outer iteration.
//! - Reports throughput via `Throughput::Elements(100)` so criterion
//!   emits per-snippet throughput in elements/second.
//!
//! The 100-element count is load-bearing — the parser script
//! `scripts/bench-embed-throughput.sh` computes
//! `cpu_emb_per_sec = 100 / mean_seconds_per_iter` and asserts
//! `cpu_emb_per_sec >= 50` per the master-plan target.  Mutating
//! [`SNIPPETS`] to fewer than 100 elements (or [`Throughput::Elements`]
//! to a different count) breaks the parser's contract — both
//! mutations are tested by the WO's pre-baked mutation block.
//!
//! # Why no [`crate::OnnxSession`] composition
//!
//! Per `WO-0059` `models.rs` upstream-fit divergence, [`CodeRankEmbed`]
//! loads `ort::Session` directly to handle dual-input
//! (`input_ids` + `attention_mask`).  This bench inherits that
//! decision — it constructs a [`CodeRankEmbed`] via [`CodeRankEmbed::load`]
//! and exercises the public [`CodeRankEmbed::embed`] surface only.
//! Real `ort` + `tokenizers` + `CodeRankEmbed` against the production
//! 137M-parameter Int8 model — per `.claude/rules/rust-style.md`,
//! these collaborators are exercised end-to-end.
//!
//! # Frozen identifier
//!
//! The criterion `bench_function` literal name is `embed_100_snippets`.
//! `scripts/bench-embed-throughput.sh` reads
//! `target/criterion/embed_100_snippets/embed_100_snippets/new/estimates.json`;
//! changing the bench function name breaks the parser script.

use std::path::PathBuf;

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use ucil_embeddings::CodeRankEmbed;

/// 100 representative code snippets for the throughput benchmark.
///
/// Mix:
/// - 30 Rust functions / structs / impls;
/// - 25 Python defs / classes / decorators;
/// - 25 TypeScript functions / interfaces / classes;
/// - 20 plain-text doc-comment-style blocks.
///
/// Each snippet is between ~50 and ~400 bytes — small enough that
/// `CodeRankEmbed`'s tokenizer produces well under the 8192-token
/// context cap, large enough to exercise realistic per-call CPU work
/// (~5-15 ms on the reference 137M Int8 model).
///
/// The 100-element count is load-bearing: see file-level rustdoc.
pub const SNIPPETS: [&str; 100] = [
    // ── Rust (30) ────────────────────────────────────────────────
    "fn add(a: i32, b: i32) -> i32 { a + b }",
    "fn factorial(n: u64) -> u64 { (1..=n).product() }",
    "fn is_prime(n: u32) -> bool { (2..n).all(|i| n % i != 0) && n > 1 }",
    "struct Point { x: f64, y: f64 }",
    "impl Point { fn distance(&self, other: &Point) -> f64 { ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt() } }",
    "enum Color { Red, Green, Blue, Rgb(u8, u8, u8) }",
    "trait Shape { fn area(&self) -> f64; fn perimeter(&self) -> f64; }",
    "fn fibonacci(n: u32) -> u64 { if n < 2 { n as u64 } else { fibonacci(n - 1) + fibonacci(n - 2) } }",
    "use std::collections::HashMap; fn count_words(text: &str) -> HashMap<String, u32> { let mut counts = HashMap::new(); for word in text.split_whitespace() { *counts.entry(word.to_string()).or_insert(0) += 1; } counts }",
    "async fn fetch(url: &str) -> Result<String, reqwest::Error> { let body = reqwest::get(url).await?.text().await?; Ok(body) }",
    "fn quicksort<T: Ord + Clone>(arr: &[T]) -> Vec<T> { if arr.len() <= 1 { return arr.to_vec(); } let pivot = arr[0].clone(); let less: Vec<T> = arr[1..].iter().filter(|x| **x < pivot).cloned().collect(); quicksort(&less) }",
    "#[derive(Debug, Clone, PartialEq)] pub struct User { pub id: u64, pub name: String, pub email: String }",
    "impl Default for User { fn default() -> Self { Self { id: 0, name: String::new(), email: String::new() } } }",
    "fn parse_int(s: &str) -> Result<i64, std::num::ParseIntError> { s.trim().parse::<i64>() }",
    "fn merge_sort<T: Ord + Clone>(arr: &[T]) -> Vec<T> { if arr.len() <= 1 { return arr.to_vec(); } let mid = arr.len() / 2; merge(&merge_sort(&arr[..mid]), &merge_sort(&arr[mid..])) }",
    "pub trait Repository<T> { fn save(&mut self, item: T) -> Result<u64, RepoError>; fn find(&self, id: u64) -> Option<&T>; fn delete(&mut self, id: u64) -> Result<(), RepoError>; }",
    "fn binary_search<T: Ord>(arr: &[T], target: &T) -> Option<usize> { let mut low = 0; let mut high = arr.len(); while low < high { let mid = low + (high - low) / 2; if arr[mid] == *target { return Some(mid); } else if arr[mid] < *target { low = mid + 1; } else { high = mid; } } None }",
    "fn read_file(path: &std::path::Path) -> std::io::Result<String> { std::fs::read_to_string(path) }",
    "use tokio::sync::Mutex; struct Counter { value: Mutex<u64> }",
    "impl Counter { pub async fn increment(&self) { let mut value = self.value.lock().await; *value += 1; } }",
    "fn map_pairs<K, V, F, R>(map: &std::collections::HashMap<K, V>, f: F) -> Vec<R> where K: std::hash::Hash + Eq, F: Fn(&K, &V) -> R { map.iter().map(|(k, v)| f(k, v)).collect() }",
    "#[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> { let body = reqwest::get(\"https://example.com\").await?.text().await?; println!(\"{body}\"); Ok(()) }",
    "fn longest_common_prefix(strs: &[String]) -> String { if strs.is_empty() { return String::new(); } let first = &strs[0]; for (i, c) in first.char_indices() { for s in &strs[1..] { if s.chars().nth(i) != Some(c) { return first[..i].to_string(); } } } first.clone() }",
    "pub fn levenshtein(a: &str, b: &str) -> usize { let n = a.chars().count(); let m = b.chars().count(); if n == 0 { return m; } if m == 0 { return n; } let mut dp = vec![vec![0; m + 1]; n + 1]; for i in 0..=n { dp[i][0] = i; } for j in 0..=m { dp[0][j] = j; } dp[n][m] }",
    "trait Iterator { type Item; fn next(&mut self) -> Option<Self::Item>; fn count(self) -> usize where Self: Sized { self.fold(0, |c, _| c + 1) } }",
    "fn group_by<T, K, F>(items: Vec<T>, key_fn: F) -> std::collections::HashMap<K, Vec<T>> where K: std::hash::Hash + Eq, F: Fn(&T) -> K { let mut groups: std::collections::HashMap<K, Vec<T>> = std::collections::HashMap::new(); for item in items { groups.entry(key_fn(&item)).or_default().push(item); } groups }",
    "#[derive(thiserror::Error, Debug)] pub enum AppError { #[error(\"io: {0}\")] Io(#[from] std::io::Error), #[error(\"parse: {0}\")] Parse(#[from] std::num::ParseIntError), #[error(\"not found: {0}\")] NotFound(String) }",
    "pub async fn retry<F, Fut, T, E>(mut f: F, max_attempts: u32) -> Result<T, E> where F: FnMut() -> Fut, Fut: std::future::Future<Output = Result<T, E>> { let mut attempts = 0; loop { match f().await { Ok(v) => return Ok(v), Err(e) if attempts + 1 >= max_attempts => return Err(e), Err(_) => attempts += 1 } } }",
    "fn rotate_left<T: Clone>(arr: &[T], k: usize) -> Vec<T> { let n = arr.len(); if n == 0 { return Vec::new(); } let k = k % n; let mut result = Vec::with_capacity(n); result.extend_from_slice(&arr[k..]); result.extend_from_slice(&arr[..k]); result }",
    "#[cfg(test)] mod tests { use super::*; #[test] fn test_add() { assert_eq!(add(2, 3), 5); } #[test] fn test_factorial_zero() { assert_eq!(factorial(0), 1); } }",
    // ── Python (25) ──────────────────────────────────────────────
    "def fibonacci(n: int) -> int:\n    if n < 2:\n        return n\n    return fibonacci(n - 1) + fibonacci(n - 2)",
    "def is_palindrome(s: str) -> bool:\n    cleaned = ''.join(c.lower() for c in s if c.isalnum())\n    return cleaned == cleaned[::-1]",
    "class Stack:\n    def __init__(self):\n        self._items = []\n    def push(self, item):\n        self._items.append(item)\n    def pop(self):\n        return self._items.pop() if self._items else None",
    "def quicksort(arr):\n    if len(arr) <= 1:\n        return arr\n    pivot = arr[0]\n    less = [x for x in arr[1:] if x < pivot]\n    greater = [x for x in arr[1:] if x >= pivot]\n    return quicksort(less) + [pivot] + quicksort(greater)",
    "@dataclass\nclass Point:\n    x: float\n    y: float\n    def distance(self, other: 'Point') -> float:\n        return ((self.x - other.x) ** 2 + (self.y - other.y) ** 2) ** 0.5",
    "async def fetch_url(url: str) -> str:\n    async with aiohttp.ClientSession() as session:\n        async with session.get(url) as response:\n            return await response.text()",
    "def merge_sort(arr):\n    if len(arr) <= 1:\n        return arr\n    mid = len(arr) // 2\n    left = merge_sort(arr[:mid])\n    right = merge_sort(arr[mid:])\n    return merge(left, right)",
    "from typing import TypeVar, Generic\nT = TypeVar('T')\nclass Box(Generic[T]):\n    def __init__(self, value: T):\n        self._value = value\n    def get(self) -> T:\n        return self._value",
    "@functools.lru_cache(maxsize=None)\ndef ackermann(m: int, n: int) -> int:\n    if m == 0:\n        return n + 1\n    if n == 0:\n        return ackermann(m - 1, 1)\n    return ackermann(m - 1, ackermann(m, n - 1))",
    "def count_words(text: str) -> dict[str, int]:\n    counts: dict[str, int] = {}\n    for word in text.split():\n        counts[word] = counts.get(word, 0) + 1\n    return counts",
    "class BinaryTree:\n    def __init__(self, value, left=None, right=None):\n        self.value = value\n        self.left = left\n        self.right = right\n    def in_order(self):\n        if self.left:\n            yield from self.left.in_order()\n        yield self.value\n        if self.right:\n            yield from self.right.in_order()",
    "import contextlib\n@contextlib.contextmanager\ndef timer(label):\n    import time\n    start = time.perf_counter()\n    try:\n        yield\n    finally:\n        elapsed = time.perf_counter() - start\n        print(f'{label}: {elapsed:.6f}s')",
    "def levenshtein(a: str, b: str) -> int:\n    if len(a) < len(b):\n        return levenshtein(b, a)\n    if not b:\n        return len(a)\n    previous = list(range(len(b) + 1))\n    for i, ca in enumerate(a):\n        current = [i + 1]\n        for j, cb in enumerate(b):\n            current.append(min(previous[j + 1] + 1, current[j] + 1, previous[j] + (ca != cb)))\n        previous = current\n    return previous[-1]",
    "def group_by(items, key_fn):\n    groups = {}\n    for item in items:\n        groups.setdefault(key_fn(item), []).append(item)\n    return groups",
    "import asyncio\nasync def producer(queue, items):\n    for item in items:\n        await queue.put(item)\n    await queue.put(None)\nasync def consumer(queue):\n    while True:\n        item = await queue.get()\n        if item is None:\n            break\n        process(item)",
    "class APIError(Exception):\n    def __init__(self, status_code: int, message: str):\n        super().__init__(message)\n        self.status_code = status_code",
    "def flatten(nested):\n    for x in nested:\n        if isinstance(x, (list, tuple)):\n            yield from flatten(x)\n        else:\n            yield x",
    "def memoize(fn):\n    cache = {}\n    def wrapper(*args):\n        if args not in cache:\n            cache[args] = fn(*args)\n        return cache[args]\n    return wrapper",
    "@pytest.mark.asyncio\nasync def test_concurrent_fetch():\n    urls = ['http://a.test', 'http://b.test']\n    results = await asyncio.gather(*[fetch_url(u) for u in urls])\n    assert len(results) == 2",
    "def matrix_multiply(a, b):\n    rows_a, cols_a = len(a), len(a[0])\n    rows_b, cols_b = len(b), len(b[0])\n    if cols_a != rows_b:\n        raise ValueError('incompatible')\n    return [[sum(a[i][k] * b[k][j] for k in range(cols_a)) for j in range(cols_b)] for i in range(rows_a)]",
    "from typing import Protocol\nclass Drawable(Protocol):\n    def draw(self, canvas) -> None:\n        ...\n    def bounds(self) -> tuple[int, int, int, int]:\n        ...",
    "def parse_json(text):\n    import json\n    try:\n        return json.loads(text)\n    except json.JSONDecodeError as e:\n        raise ValueError(f'invalid JSON: {e}') from e",
    "class CircularBuffer:\n    def __init__(self, capacity: int):\n        self._buf = [None] * capacity\n        self._capacity = capacity\n        self._head = 0\n        self._size = 0\n    def push(self, item):\n        self._buf[(self._head + self._size) % self._capacity] = item",
    "def topo_sort(graph):\n    in_degree = {n: 0 for n in graph}\n    for n in graph:\n        for m in graph[n]:\n            in_degree[m] = in_degree.get(m, 0) + 1\n    queue = [n for n, d in in_degree.items() if d == 0]\n    order = []\n    while queue:\n        n = queue.pop(0)\n        order.append(n)\n    return order",
    "@app.route('/api/users/<int:user_id>')\ndef get_user(user_id):\n    user = User.query.get_or_404(user_id)\n    return jsonify(user.to_dict())",
    // ── TypeScript (25) ──────────────────────────────────────────
    "function add(a: number, b: number): number { return a + b; }",
    "const isEven = (n: number): boolean => n % 2 === 0;",
    "interface User { id: number; name: string; email: string; }",
    "class Stack<T> { private items: T[] = []; push(item: T): void { this.items.push(item); } pop(): T | undefined { return this.items.pop(); } }",
    "async function fetchUser(id: number): Promise<User> { const response = await fetch(`/api/users/${id}`); return response.json(); }",
    "type Result<T, E> = { ok: true; value: T } | { ok: false; error: E };",
    "function map<T, U>(arr: T[], fn: (item: T) => U): U[] { const result: U[] = []; for (const item of arr) { result.push(fn(item)); } return result; }",
    "enum Color { Red = 'red', Green = 'green', Blue = 'blue' }",
    "class Observable<T> { private subscribers: Array<(value: T) => void> = []; subscribe(fn: (value: T) => void): () => void { this.subscribers.push(fn); return () => { this.subscribers = this.subscribers.filter(s => s !== fn); }; } }",
    "function quickSort<T>(arr: T[], compare: (a: T, b: T) => number): T[] { if (arr.length <= 1) return arr; const [pivot, ...rest] = arr; const less = rest.filter(x => compare(x, pivot) < 0); const greater = rest.filter(x => compare(x, pivot) >= 0); return [...quickSort(less, compare), pivot, ...quickSort(greater, compare)]; }",
    "interface Repository<T, K> { save(item: T): Promise<K>; find(id: K): Promise<T | null>; delete(id: K): Promise<void>; }",
    "const debounce = <F extends (...args: any[]) => void>(fn: F, ms: number): F => { let timer: ReturnType<typeof setTimeout>; return ((...args: any[]) => { clearTimeout(timer); timer = setTimeout(() => fn(...args), ms); }) as F; };",
    "class HttpClient { constructor(private baseUrl: string) {} async get<T>(path: string): Promise<T> { const r = await fetch(`${this.baseUrl}${path}`); if (!r.ok) throw new Error(r.statusText); return r.json(); } }",
    "function memoize<Args extends unknown[], R>(fn: (...args: Args) => R): (...args: Args) => R { const cache = new Map<string, R>(); return (...args: Args): R => { const key = JSON.stringify(args); if (!cache.has(key)) cache.set(key, fn(...args)); return cache.get(key)!; }; }",
    "type ReadonlyDeep<T> = { readonly [P in keyof T]: T[P] extends object ? ReadonlyDeep<T[P]> : T[P]; };",
    "export class EventEmitter<E extends Record<string, unknown>> { private handlers: Map<keyof E, Set<(payload: any) => void>> = new Map(); on<K extends keyof E>(event: K, handler: (payload: E[K]) => void): void { if (!this.handlers.has(event)) this.handlers.set(event, new Set()); this.handlers.get(event)!.add(handler); } }",
    "function partition<T>(arr: T[], predicate: (item: T) => boolean): [T[], T[]] { const yes: T[] = []; const no: T[] = []; for (const item of arr) { (predicate(item) ? yes : no).push(item); } return [yes, no]; }",
    "interface Listener<T> { (event: T): void; }\nfunction createEvent<T>(): { fire: (e: T) => void; on: (l: Listener<T>) => void } { const listeners: Listener<T>[] = []; return { fire: e => listeners.forEach(l => l(e)), on: l => listeners.push(l) }; }",
    "async function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T> { return Promise.race([promise, new Promise<T>((_, reject) => setTimeout(() => reject(new Error('timeout')), ms))]); }",
    "describe('User', () => { it('should have a name', () => { const u: User = { id: 1, name: 'Alice', email: 'a@b.c' }; expect(u.name).toBe('Alice'); }); });",
    "type Brand<K, T> = K & { __brand: T };\ntype UserId = Brand<number, 'UserId'>;\ntype OrderId = Brand<number, 'OrderId'>;",
    "const pick = <T extends object, K extends keyof T>(obj: T, keys: K[]): Pick<T, K> => keys.reduce((r, k) => ({ ...r, [k]: obj[k] }), {} as Pick<T, K>);",
    "export async function* paginate<T>(fetchPage: (cursor?: string) => Promise<{ items: T[]; next?: string }>, initial?: string): AsyncGenerator<T> { let cursor = initial; do { const page = await fetchPage(cursor); for (const item of page.items) yield item; cursor = page.next; } while (cursor !== undefined); }",
    "class LRUCache<K, V> { private cache: Map<K, V> = new Map(); constructor(private capacity: number) {} get(key: K): V | undefined { if (!this.cache.has(key)) return undefined; const v = this.cache.get(key)!; this.cache.delete(key); this.cache.set(key, v); return v; } }",
    "export interface Config { apiUrl: string; timeoutMs: number; retries: number; logger?: { info(msg: string): void; warn(msg: string): void; error(msg: string, err?: unknown): void; }; }",
    // ── Plain text / doc-comment-style (20) ──────────────────────
    "Authentication is handled via OAuth 2.0 with PKCE flow. The access token expires after one hour and must be refreshed using the refresh token endpoint.",
    "The cache invalidation strategy uses LRU with a maximum of 10000 entries. Entries older than 24 hours are evicted regardless of access frequency.",
    "Database migrations run automatically on application startup. Each migration is wrapped in a transaction; failures roll back cleanly without partial state.",
    "The rate limiter uses a sliding-window algorithm bucketed per API key. Default limit is 1000 requests per minute, configurable via the `RATE_LIMIT` environment variable.",
    "Error responses follow RFC 7807 problem-details format. Each response includes `type`, `title`, `status`, `detail`, and an `instance` URL that uniquely identifies the failure.",
    "The deployment pipeline triggers on every push to `main` and runs unit tests, integration tests, security scans, and a canary deployment to staging before production.",
    "Logs are structured as JSON with fields `timestamp`, `level`, `service`, `trace_id`, and `message`. Trace IDs propagate across service boundaries via W3C Trace Context.",
    "The service mesh enforces mTLS between all internal services. Certificates rotate every 24 hours via SPIFFE; clients without valid SVIDs are rejected at the proxy layer.",
    "Configuration values are loaded from `config.yaml`, then overridden by environment variables prefixed `APP_`. Missing required values cause startup to fail with an actionable message.",
    "The event bus uses Apache Kafka with three brokers per region. Partitioning is by user ID; producers are idempotent; consumers commit offsets after successful processing.",
    "Backups run nightly at 02:00 UTC and are retained for 30 days. Cross-region replication ensures durability; restore time objective is 4 hours, recovery point objective is 1 hour.",
    "API versioning uses URL path segments (e.g., `/v1/users`). Deprecated versions remain supported for 12 months after a successor version is released.",
    "The frontend bundle is split into core, vendor, and route chunks. Initial page load weighs 120 KB gzipped; subsequent route loads stream in lazily.",
    "Feature flags are managed via LaunchDarkly with a 30-second TTL on cache. Flags can be evaluated server-side or client-side; user attributes are normalized to a stable hash.",
    "The audit log is append-only and tamper-evident. Each entry's hash chains to the previous entry; periodic checkpoints are signed by the offline auditor key.",
    "Monitoring dashboards are built in Grafana with PromQL queries against the central Mimir instance. SLOs are defined in code and reviewed quarterly by the SRE team.",
    "The recommendation model is retrained weekly from event data older than 7 days but newer than 90 days. A/B testing compares against the previous champion model before promotion.",
    "Search relevance is computed using BM25 with field weights tuned per domain. The query analyzer applies stemming, stop-word removal, and synonym expansion before scoring.",
    "Rate-limited endpoints return HTTP 429 with `Retry-After` headers. Clients should implement exponential backoff with jitter, capped at 60 seconds between retries.",
    "Documentation lives next to code in Markdown files and is rendered via MkDocs. Cross-references use relative paths so the docs site can be built offline from a tarball.",
];

/// Walk up from `CARGO_MANIFEST_DIR` to find `ml/models/coderankembed/`.
///
/// Mirrors the upward-walk shape used by `models.rs:test_coderankembed_inference`
/// (lines 441-463) so the bench resolves the production model directory
/// the same way as the frozen acceptance test.  `panic!`s with an
/// operator-readable message when the directory is absent — the bench
/// is gated by the verify script's pre-flight `install-coderankembed.sh`
/// invocation, so this panic only fires when an operator runs the bench
/// outside of `scripts/bench-embed-throughput.sh`.
fn locate_model_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .expect("crates/ parent of ucil-embeddings exists")
        .parent()
        .expect("workspace root parent of crates/ exists");
    let model_dir = repo_root.join("ml").join("models").join("coderankembed");
    let model_onnx = model_dir.join("model.onnx");
    let tokenizer_json = model_dir.join("tokenizer.json");
    if !model_onnx.exists() || !tokenizer_json.exists() {
        panic!(
            "ml/models/coderankembed/ not found — run scripts/devtools/install-coderankembed.sh first \
             (P2-W8-F06 / WO-0061); model.onnx exists={}, tokenizer.json exists={}",
            model_onnx.exists(),
            tokenizer_json.exists(),
        );
    }
    model_dir
}

/// The criterion bench function — exercises [`CodeRankEmbed::embed`]
/// over [`SNIPPETS`] (100 elements) per outer iteration.
///
/// `Throughput::Elements(100)` matches the inner-loop count so criterion
/// reports per-snippet throughput.  `sample_size(10)` is a deliberate
/// reduction from criterion's default 100 — at 100 model calls per
/// outer iteration × ~5-15 ms per call, even 10 samples take 5-15 s
/// each, totalling ~1-3 minutes of bench wall-time.  The default 100
/// samples would push that to 10-30 minutes which exceeds the AC40
/// 5-minute verifier wall-time budget.
///
/// `black_box` is applied to both the input snippet and the embed
/// result so the optimiser cannot eliminate the call (for static-input
/// bench loops criterion's docs recommend bracketing both ends).
fn bench_embed_100_snippets(c: &mut Criterion) {
    let model_dir = locate_model_dir();
    let mut model =
        CodeRankEmbed::load(&model_dir).expect("CodeRankEmbed::load on ml/models/coderankembed");

    let mut group = c.benchmark_group("embed_100_snippets");
    group.throughput(Throughput::Elements(SNIPPETS.len() as u64));
    group.sample_size(10);
    group.bench_function("embed_100_snippets", |b| {
        b.iter(|| {
            for snippet in SNIPPETS.iter() {
                let _ = black_box(
                    model
                        .embed(black_box(snippet))
                        .expect("CodeRankEmbed::embed on baked-in snippet"),
                );
            }
        });
    });
    group.finish();
}

criterion_group!(benches, bench_embed_100_snippets);
criterion_main!(benches);
