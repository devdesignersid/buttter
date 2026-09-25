# Rust Code-Smell and Anti-Pattern Reference

This is the repository's required reference for writing and reviewing Rust. Its goals are correctness, soundness, clear ownership, productive maintenance, and measured performance.

No finite checklist can identify every bad design, and many Rust choices depend on workload and context. Treat entries marked **Avoid** as strong defaults, not substitutes for engineering judgment. Treat entries marked **Measure** as hypotheses that require representative evidence. When requirements conflict, preserve correctness first, then measure the simplest correct design.

## How to use this reference

Each rule has one or more enforcement labels:

- **Compiler**: `rustc` normally rejects or diagnoses the problem.
- **Clippy gate**: the repository's required Clippy invocation detects the listed lint.
- **Test gate**: automated tests, coverage, or mutation testing should detect the behavior.
- **Review**: a reviewer must evaluate context; simple textual bans would be unreliable.
- **Measure**: accept a performance claim only with a reproducible benchmark or profile representative of the actual workload.

Severity terms:

- **Never**: the pattern is unsound, incorrect, or prohibited unless a separately approved requirement explicitly demands it.
- **Avoid**: use a clearer or safer alternative unless a local, documented reason applies.
- **Consider**: investigate when the stated conditions occur; it is not an automatic defect.

For a justified lint exception, use the narrowest item-level `#[expect(clippy::lint_name, reason = "...")]`. Do not use crate-wide suppression, lint-group suppression, or an unexplained `#[allow(...)]`. An expectation must state the invariant or requirement that makes the flagged code appropriate. Remove stale expectations.

## Mechanically enforced baseline

Every tracked Rust source must belong to a target in `.github/quality-targets.toml`. The required `Repository quality` gate runs Rustfmt, Clippy, tests with 100% repository-authored product line coverage, and coverage-report validation. A separate affected-module mutation gate applies to changed product behavior.

The Clippy command denies compiler warnings, `clippy::all`, and these additional high-signal lints:

- `clippy::dbg_macro`
- `clippy::todo`
- `clippy::unimplemented`
- `clippy::undocumented_unsafe_blocks`
- `clippy::multiple_unsafe_ops_per_block`
- `clippy::await_holding_lock`
- `clippy::large_futures`
- `clippy::large_stack_arrays`
- `clippy::large_types_passed_by_value`
- `clippy::rc_buffer`
- `clippy::mutex_atomic`
- `clippy::zombie_processes`

The whole `pedantic`, `nursery`, and `restriction` groups are intentionally not denied. They include context-sensitive and sometimes conflicting advice. Review their useful findings individually rather than assuming every lint is project policy.

Static analysis cannot prove that an abstraction is useful, a clone is too expensive, a lock is too broad, an error has enough context, or a data structure fits real traffic. Those remain mandatory review items.

## 1. Correctness and explicit invariants

### 1.1 Ignoring a meaningful `Result`

**Never** discard an operation that can fail with `let _ = operation()`, `.ok()`, an empty `Err` arm, or an ignored join result merely to silence `must_use`. This hides incomplete writes, failed cleanup, closed channels, panicked tasks, and corrupted state. **Compiler, Test gate, Review.**

Prefer `?`, an explicit recovery path, or a deliberately logged and documented best-effort policy. If failure is genuinely impossible, encode and explain the invariant at the narrowest boundary.

### 1.2 Unjustified `unwrap`, `expect`, indexing, and panics

**Avoid** `unwrap()`, `expect()`, `panic!`, `assert!`, direct indexing, and unreachable assumptions on runtime-controlled data. They turn recoverable input, I/O, state, and dependency failures into process or task failure. **Review, Test gate.**

They can be appropriate in tests, static initialization, and after a locally visible proof. Prefer pattern matching, `?`, checked access, and error context. An `expect` message should describe the invariant, not repeat “failed.” Never use `unwrap_unchecked` as a faster `unwrap` without proof and measurement.

### 1.3 Placeholder behavior in a reachable path

**Never** merge reachable `todo!()`, `unimplemented!()`, placeholder success values, or silent no-op branches. A compile-success placeholder is still missing behavior. **Clippy gate, Test gate.**

Use a typed unsupported error only when unsupported behavior is an approved part of the contract.

### 1.4 Arithmetic assumptions

**Avoid** relying on debug-only overflow checks, lossy `as` conversions, unchecked narrowing, division without zero handling, or signed/unsigned wraparound. Release and debug behavior must not accidentally differ. **Compiler, Clippy gate, Test gate, Review.**

Use `TryFrom`, `checked_*`, `saturating_*`, or `wrapping_*` according to the domain. State why saturation or wrapping is semantically correct. Test zero, minimum, maximum, and just-outside boundaries.

### 1.5 Floating-point equality and ordering

**Avoid** exact equality for computed floats, unchecked NaN assumptions, and pretending `f32`/`f64` has total ordinary ordering. `partial_cmp` can return `None`; NaN can poison geometry and sorting. **Review, Test gate.**

Define domain tolerances, validate finite values at boundaries, or use `total_cmp` when its total ordering is the intended contract. Do not choose an epsilon without considering scale.

### 1.6 Invalid or ambiguous coordinates, units, and identifiers

**Avoid** passing raw numeric primitives for screen/world coordinates, pixels, zoom, angles, durations, byte counts, indices, and unrelated IDs. Swapped values compile and unit conversion becomes implicit. **Review.**

Use small newtypes and explicit conversion functions when confusion is plausible. Validate nonzero, positive, finite, or bounded invariants in constructors.

### 1.7 Invalid states represented as ordinary values

**Avoid** boolean clusters, sentinel numbers, magic strings, and combinations of `Option` fields that admit impossible states. **Review, Test gate.**

Use enums and validated types so each state carries only its valid data. Do not add a type-state hierarchy when a simple enum is sufficient.

### 1.8 Partial mutation before fallible work completes

**Avoid** mutating several fields or external resources and returning midway on error without a defined rollback or resumable state. **Review, Test gate.**

Validate first, compute a replacement before swapping, or use an explicit transaction/state transition. Test every failure point, including retry behavior.

### 1.9 Accidental nondeterminism

**Avoid** depending on `HashMap`/`HashSet` iteration order, filesystem enumeration order, task scheduling, locale, wall-clock timing, or randomized hashing in user-visible output and tests. **Review, Test gate.**

Sort at the boundary, use an ordered collection when ordering is part of the model, inject clocks/randomness, and make tie-breaking explicit. Do not replace a hash map with a tree map solely for deterministic tests if sorting output is cheaper and clearer.

### 1.10 Unicode and byte confusion

**Never** assume a string index is a character index. **Avoid** using bytes for user-visible text boundaries or `chars().nth(n)` repeatedly in loops. Grapheme clusters, scalar values, and UTF-8 bytes are different concepts. **Compiler, Review, Measure.**

Choose and document the required unit. Use byte operations only for byte protocols or proven ASCII. Use an appropriate Unicode segmentation library only when the product requires grapheme behavior.

### 1.11 Time confusion

**Avoid** using wall-clock time for elapsed durations, comparing timestamps from incompatible clocks, ad hoc unit conversion, and tests that sleep until “enough time” passed. **Review, Test gate.**

Use monotonic `Instant` for elapsed time, `Duration` for spans, an injected clock for deterministic domain tests, and explicit timezone semantics for civil time.

### 1.12 Recursive destruction or traversal on unbounded input

**Consider** stack exhaustion when recursively processing user-sized trees, graphs, parser nesting, or linked structures. **Review, Test gate, Measure.**

Use an explicit stack or bounded depth when input can be adversarial or very deep. Recursion is fine for structurally small data.

### 1.13 Side effects hidden in `Drop`

**Avoid** making required persistence, network delivery, or observable success depend only on `Drop`. Destructors cannot return errors, may run during unwinding, and are not guaranteed after process abort. **Review, Test gate.**

Provide an explicit `close`, `flush`, or `commit` that returns `Result`; reserve `Drop` for best-effort cleanup and invariant preservation.

### 1.14 Resource leaks through cycles or forgotten guards

**Avoid** strong `Rc`/`Arc` cycles, `mem::forget` as ordinary control flow, leaked task handles, and guards whose lifetime accidentally spans unrelated work. **Review, Test gate.**

Use `Weak` for back-references, explicit ownership trees, scoped guards, and explicit shutdown. Leak only when process-lifetime allocation is an intentional, bounded design.

## 2. Ownership, borrowing, and lifetimes

### 2.1 Cloning to appease the borrow checker

**Avoid** adding `.clone()` before understanding which ownership or borrow overlaps are required. It can hide poor boundaries and introduce allocation, reference-count traffic, or stale snapshots. **Review, Measure.**

First shorten borrow scopes, split fields, borrow a slice/view, move ownership, or reorder validation and mutation. Keep a clone when ownership duplication is semantically required and its cost is acceptable.

### 2.2 Returning owned data when a borrow is sufficient

**Avoid** allocating `String`, `Vec`, or copied structures from accessors when the owner can safely return `&str`, `&[T]`, an iterator, or a domain view. **Review, Measure.**

Do not expose a borrow that freezes internal representation or makes callers fight lifetimes. Ownership can be the right API boundary.

### 2.3 Accepting concrete owned containers unnecessarily

**Avoid** requiring `&String`, `&Vec<T>`, `&Box<T>`, or owned values when the function only reads `&str`, `&[T]`, `&T`, `impl AsRef<Path>`, or an iterator. **Clippy gate, Review.**

Prefer the least restrictive interface that accurately states use. Avoid generic `AsRef`/`Into` everywhere; excessive generic APIs increase compile time and can obscure errors.

### 2.4 Overusing `Cow`

**Avoid** `Cow` when behavior is always borrowed or always owned, or when callers cannot benefit from copy-on-write. **Review, Measure.**

Use it where both paths are common and avoiding allocation matters. Otherwise return a clear borrowed or owned type.

### 2.5 Reference counting without shared ownership

**Avoid** defaulting to `Rc`, `Arc`, `Arc<Mutex<_>>`, or cloning handles for convenience. Reference counting adds indirection and runtime bookkeeping and obscures the lifetime of shared state. **Review, Measure.**

Prefer a single owner and borrows, message passing, or scoped tasks. Use `Rc` for genuine single-thread shared ownership and `Arc` only when ownership crosses threads/tasks or an API requires it.

### 2.6 `Arc` around data that is already cheaply owned

**Avoid** wrapping tiny `Copy` values or short immutable configuration in `Arc` without a demonstrated ownership need. **Review, Measure.**

Copy or clone cheap immutable values. Conversely, do not clone a large immutable structure repeatedly merely to avoid appropriate shared ownership.

### 2.7 Excessive interior mutability

**Avoid** broad `Cell`, `RefCell`, `Mutex`, `RwLock`, and atomics used to bypass an unclear ownership model. Runtime borrow failures and lock contention replace compile-time reasoning. **Review, Test gate, Measure.**

Keep mutable state small, private, and protected by one clear synchronization policy. Prefer actor/message ownership where it simplifies state transitions.

### 2.8 Long-lived or overly broad borrows

**Avoid** retaining references, lock guards, iterator borrows, or entry guards across expensive computation, callbacks, I/O, `.await`, or unrelated mutations. **Compiler, Clippy gate, Review.**

Extract the needed value, end the scope, and then perform the expensive or re-entrant work.

### 2.9 Self-referential structures without necessity

**Avoid** self-references, pinning, raw-pointer back-links, and arenas introduced only to avoid ordinary ownership design. **Review.**

Prefer indices, stable IDs, owned nodes, or established arena crates when requirements justify them. Self-referential unsafe code has a large invariant surface.

### 2.10 Needless explicit lifetimes or lifetime coupling

**Avoid** naming lifetimes the compiler can elide and using one lifetime for independent inputs/outputs. **Clippy gate, Review.**

Let elision communicate the common case. Name lifetimes when they explain an actual relationship. Do not use `'static` to “fix” a borrow error by leaking data or overconstraining callers.

### 2.11 `Deref` as inheritance or implicit conversion

**Avoid** implementing `Deref` merely to expose another type's methods or emulate inheritance. It creates a large implicit API and surprising method resolution. **Review.**

Use named accessors, composition, and trait implementations. `Deref` is appropriate for pointer-like smart-pointer behavior with stable target semantics.

### 2.12 `Box<T>` without a reason

**Avoid** boxing small values merely to make the compiler accept a design. **Review, Measure.**

Use `Box` for recursive types, trait objects, ownership erasure, stable addresses, or measured stack/layout needs. Boxing adds allocation and indirection.

## 3. Type and API design

### 3.1 Stringly typed domain models

**Avoid** strings for finite states, commands, property names, file kinds, IDs, or validated input. Typos become runtime behavior and refactoring loses compiler support. **Review, Test gate.**

Use enums, newtypes, and validated parsers. Preserve unknown external values explicitly when forward compatibility requires it.

### 3.2 Boolean blindness

**Avoid** public functions with multiple positional booleans or booleans whose meaning is unclear at the call site. **Review.**

Use an enum, options struct, or named builder. One obvious predicate remains fine.

### 3.3 Primitive obsession and parameter trains

**Avoid** long lists of related primitives and repeatedly passing the same cluster through layers. **Review.**

Group cohesive domain data. Do not create one-field wrappers or “context” bags that merely hide unrelated dependencies.

### 3.4 God structs and god modules

**Avoid** one state object that owns rendering, persistence, networking, input, history, and domain decisions, or modules that change for unrelated reasons. **Review.**

Split by cohesive responsibility and state transition. Do not split mechanically into tiny forwarding types; every abstraction must earn its boundary.

### 3.5 Public fields that bypass invariants

**Avoid** public mutable fields when values require validation or coordinated updates. **Review, Test gate.**

Use constructors and intention-revealing methods. Plain data-transfer structures may appropriately have public fields.

### 3.6 Constructors that do not establish validity

**Avoid** `new` functions that create half-initialized objects requiring a precise call sequence. **Review, Test gate.**

Require essential data in construction, represent staged state explicitly, or return `Result`. A builder is useful for many optional settings, not as ceremony for two required fields.

### 3.7 Getters and setters that erase behavior

**Avoid** mechanically exposing every field and letting callers coordinate invariants. **Review.**

Expose domain operations. A simple accessor is fine when reading the value is the actual contract.

### 3.8 Broad public APIs and premature stability

**Avoid** making modules, traits, fields, generics, and extension hooks public “for future use.” Public surface constrains refactoring and increases documentation/testing obligations. **Review.**

Start private or crate-visible and expose the smallest required contract. Use `#[non_exhaustive]` only when external evolution is a real requirement.

### 3.9 Weak return types

**Avoid** returning tuples with unclear positions, booleans that discard failure reasons, `Option` when absence and invalidity differ, or `Result<T, String>` across durable boundaries. **Review.**

Use named structures and typed errors. A local tuple or simple string error can be appropriate when its scope is truly narrow.

### 3.10 Misusing conversion traits

**Avoid** fallible or lossy `From`, surprising `Into`, expensive implicit conversions, and broad conversion implementations that make APIs ambiguous. **Review, Test gate.**

`From` should be infallible and unsurprising; use `TryFrom` for validation and named methods for lossy or policy-dependent conversion.

### 3.11 Inappropriate `Default`

**Avoid** implementing `Default` when there is no obvious valid default or when it creates an invalid placeholder. **Review.**

Use a named constructor expressing policy. Derive `Default` for genuinely neutral configuration and data values.

### 3.12 Inappropriate `Copy`, `Clone`, equality, or ordering

**Avoid** deriving semantic traits without deciding what they mean. `Copy` can make accidental duplication invisible; equality on handles or floating data can mislead; ordering may invent meaningless semantics. **Review, Test gate.**

Derive when field-wise behavior is the domain behavior. Otherwise implement deliberately or omit the trait.

### 3.13 Missing `must_use` on important values

**Consider** `#[must_use]` for builders, guards, lazy operations, and values whose ignored result almost certainly means a bug. **Compiler, Review.**

Do not mark everything; warning fatigue reduces signal.

### 3.14 Extension traits and blanket implementations that surprise users

**Avoid** broad blanket impls, generic extension traits, and method names likely to collide unless they provide substantial repeated value. **Review.**

Prefer ordinary functions or a narrow trait owned by the relevant abstraction. Respect coherence and downstream compatibility.

### 3.15 Trait objects or generics by default

**Avoid** introducing a trait for a single implementation without a concrete testing, substitution, plugin, or architecture requirement. **Review, Measure.**

Generics can improve static composition but increase monomorphization and compile time. Trait objects can reduce code size but add indirection and object-safety constraints. Choose based on the real boundary and measure performance-sensitive paths.

### 3.16 Giant enums and exhaustive matches spread everywhere

**Consider** whether a large enum whose variants force many unrelated modules to change has mixed responsibilities. **Review.**

Central exhaustive matching is often a strength. Split only when variants truly represent independent concepts; do not replace an understandable enum with trait-object indirection automatically.

## 4. Error handling and failure semantics

### 4.1 Catch-all error erasure

**Avoid** mapping distinct failures to `None`, `false`, empty output, or one opaque message. Callers lose retry, user messaging, and diagnostics. **Review, Test gate.**

Preserve source errors and classify failures that require different handling.

### 4.2 Missing context at boundaries

**Avoid** propagating low-level errors without identifying the operation and relevant safe identifier, path, or state. **Review, Test gate.**

Add context where crossing subsystem boundaries. Do not repeat the same context at every frame or expose secrets in errors.

### 4.3 Logging and returning the same error at every layer

**Avoid** log-and-propagate patterns that create duplicate events. **Review.**

Attach context while propagating; log once at the boundary that owns the recovery or user-visible failure. Log again only when recording a distinct state transition.

### 4.4 Treating all errors as fatal or all as recoverable

**Avoid** one policy for programmer defects, malformed input, transient I/O, cancellation, resource exhaustion, and invariant corruption. **Review, Test gate.**

Define which layer handles each class. Never retry permanent errors blindly, and never continue after violated safety invariants.

### 4.5 Retrying without policy

**Avoid** immediate infinite retries, synchronized retries, retrying non-idempotent operations, and dropping the final cause. **Review, Test gate, Measure.**

Specify attempt limits, backoff, jitter where applicable, cancellation, idempotency, and observability. Test exhaustion and interruption.

### 4.6 Cleanup that masks the primary failure

**Avoid** replacing the original error with a cleanup error without policy. **Review, Test gate.**

Preserve the primary failure and attach cleanup failure information, or define why cleanup failure has priority.

### 4.7 Panicking across FFI, thread, task, or callback boundaries

**Never** permit unwinding across an ABI boundary that does not allow it. **Avoid** losing thread/task panics by dropping join handles. **Compiler, Test gate, Review.**

Catch or abort according to the ABI contract, convert errors at boundaries, and inspect joins. Panic containment is not a substitute for memory-safety invariants.

### 4.8 Empty error variants and untestable messages

**Avoid** errors that lack actionable fields and tests that assert entire unstable prose strings. **Review, Test gate.**

Expose structured variants/data for behavior; test classification and relevant context. User-facing wording can be tested where wording is itself a requirement.

## 5. Collections, iterators, and algorithms

### 5.1 Wrong collection for access patterns

**Avoid** selecting `Vec`, `VecDeque`, map, set, linked list, or tree by habit. **Review, Measure.**

Start with `Vec` for compact sequential data, then choose from actual operations: front removal, key lookup, ordering, stable addresses, insertion patterns, and memory constraints.

### 5.2 `Vec::remove(0)` and repeated front insertion

**Avoid** repeatedly shifting a vector from the front. It is linear per operation and can become quadratic. **Clippy gate, Review, Measure.**

Use `VecDeque` for a queue, an index window when retaining storage, or batch removal.

### 5.3 Hidden quadratic loops

**Avoid** nested `contains`/`position` scans, repeated string concatenation, repeated `nth`, and insertion sorting on unbounded data without intent. **Clippy gate, Review, Measure.**

Use a set/map, precomputed index, `String::with_capacity`, sorting, or a better algorithm when profiling or complexity analysis shows relevance.

### 5.4 Collecting only to iterate again

**Avoid** `collect::<Vec<_>>()` used only for one immediate pass, count, membership test, or first element. **Clippy gate, Review, Measure.**

Keep iterator pipelines lazy. Collect when ownership, sorting, repeated traversal, chunking, or API boundaries require materialization.

### 5.5 Repeated lookup instead of entry APIs

**Avoid** `contains_key` followed by `get`/`insert`, or duplicate searches where `entry`, `get_mut`, or one match suffices. **Clippy gate, Review.**

Use collection entry APIs, while keeping code readable when mutation paths are complex.

### 5.6 Allocating keys for borrowed lookup

**Avoid** `map.get(&input.to_string())` and similar temporary allocation when borrowed lookup is supported. **Review, Measure.**

Use `&str`, `Borrow`, or a compatible key view. Do not create elaborate borrowed-key machinery without demonstrated need.

### 5.7 Capacity mistakes

**Avoid** repeated predictable growth and wildly speculative preallocation. **Review, Measure.**

Use `with_capacity`/`reserve` when a trustworthy bound exists. Treat untrusted length fields as hostile; cap and validate before allocation.

### 5.8 Unnecessary sorting or unstable semantics

**Avoid** sorting when only min/max/top-k is needed, cloning solely to sort, or relying on stable ordering when not required. **Review, Measure.**

Use selection, heaps, `sort_unstable`, or sort indices where appropriate. Prefer stable sort when equal-item order is part of behavior.

### 5.9 Dense indexes modeled as hash maps

**Consider** a vector or slot map when keys are compact integer-like IDs and memory locality matters. **Review, Measure.**

Do not sacrifice sparse-ID correctness or deletion safety for theoretical locality; generational IDs may be required.

### 5.10 Iterator cleverness that hides control flow

**Avoid** dense chains with side effects, nested combinators, hidden early exits, or allocations that are harder to verify than a loop. **Review.**

Use iterators when they clarify transformation; use a loop when state transitions, errors, or branching become clearer.

## 6. Allocation, layout, and performance

### 6.1 Optimizing without a representative measurement

**Never** claim a change is faster, leaner, or more scalable without a reproducible workload, build profile, toolchain, platform, and result. **Measure.**

Benchmark before and after, inspect profiles, and retain correctness tests. Microbenchmarks do not prove end-to-end impact.

### 6.2 Benchmarking debug builds or unrealistic inputs

**Avoid** performance conclusions from debug builds, one tiny input, warmed-only or cold-only caches, and benchmarks where the compiler removes the work. **Measure.**

Use the intended release profile and representative distributions. Control setup, use proper black-boxing, and report variance.

### 6.3 Allocation in hot loops

**Consider** repeated `String`, `Vec`, `Box`, formatting, and temporary collection allocation in measured hot paths. **Clippy gate, Review, Measure.**

Reuse buffers or write into existing storage when ownership remains clear. Do not pool cheap allocations without evidence; pools add retention and synchronization complexity.

### 6.4 Repeated formatting for machine data

**Avoid** formatting values into strings only to parse or compare them again. **Review, Measure.**

Keep typed values and serialize once at the boundary.

### 6.5 Large stack values and futures

**Avoid** very large local arrays/structs and async state machines that retain large values across awaits. They increase stack pressure, move cost, and task memory. **Clippy gate, Review, Measure.**

Shorten lifetimes across `.await`, split work, or box only the large portion when measurement and layout justify it.

### 6.6 Passing large values by value accidentally

**Avoid** repeatedly moving large non-`Copy` values when a borrow is sufficient. **Clippy gate, Review, Measure.**

Pass by reference or consume deliberately. Do not borrow tiny `Copy` values merely to follow a blanket rule.

### 6.7 Poor locality through pointer-heavy design

**Consider** whether trees of boxes, linked structures, trait objects, and reference-counted nodes dominate a measured traversal. **Review, Measure.**

Contiguous storage, indices, or structure-of-arrays layouts can help. Adopt them only if they preserve required mutation and identity semantics.

### 6.8 Large enum variants

**Consider** boxing an unusually large rare variant when enum size materially multiplies container or future size. **Clippy gate, Measure.**

First inspect `size_of` and profiles. Boxing every variant adds allocations and pointer chasing.

### 6.9 Copying entire buffers for small edits

**Consider** piece tables, ropes, chunked buffers, or copy-on-write only when measured edit sizes and document scale justify them. **Review, Measure.**

A simple `Vec`/`String` is often faster and easier at ordinary sizes.

### 6.10 Unnecessary reference-count churn

**Consider** frequent `Arc::clone`/drop in hot paths, especially under cross-core contention. **Review, Measure.**

Borrow within a scope, batch work, or redesign ownership. Never replace safe ownership with raw pointers merely to remove atomic increments.

### 6.11 Mutex where an atomic clearly suffices—or vice versa

**Avoid** protecting one independent primitive counter/flag with a mutex when atomic semantics are straightforward; also avoid encoding multi-field invariants in a maze of atomics. **Clippy gate, Review, Measure.**

Choose the simplest synchronization primitive that makes the memory and state model correct.

### 6.12 Manual bounds-check elimination

**Avoid** unsafe indexing, pointer loops, or obscure iterator rewrites based on an assumption that safe code is slow. **Review, Measure.**

Measure optimized assembly/profile behavior first. Give the optimizer clear safe loops and slices; use unsafe only with a demonstrated bottleneck and documented proof.

### 6.13 `#[inline(always)]` and optimization attributes by instinct

**Avoid** blanket inlining, `cold`, branch hints, target features, and link-time settings without evidence. They can increase code size and regress instruction-cache behavior or portability. **Review, Measure.**

Let the optimizer decide by default. Record benchmark evidence and target constraints for exceptions.

### 6.14 Monomorphization and compile-time blowups

**Consider** excessive generic layering, giant generated types, repeated macro expansion, and generic functions doing substantial non-generic work. **Review, Measure.**

Move shared work behind non-generic functions or use dynamic dispatch at stable boundaries when compile time/binary size is a measured problem.

### 6.15 Dynamic dispatch in a proven inner loop

**Consider** trait-object calls and boxed iterators in measured tight loops. **Measure.**

Static dispatch or enums may help, but dynamic dispatch is often negligible and can reduce code size. Do not rewrite without evidence.

### 6.16 Over-parallelizing small work

**Avoid** spawning tasks/threads or parallel iterators when scheduling, synchronization, and cache costs exceed useful work. **Review, Measure.**

Batch operations and establish a threshold from benchmarks. Preserve deterministic semantics where required.

### 6.17 Unbounded caches and retained capacity

**Avoid** caches, interning tables, arenas, channels, and reusable buffers that can grow forever under user input. **Review, Test gate, Measure.**

Define eviction, limits, ownership, and observability. `clear()` retains capacity; shrink only when retention is a measured problem.

## 7. Async Rust

### 7.1 Blocking an async executor

**Never** perform blocking file/network calls, long CPU work, synchronous waits, or thread sleeps on an async executor thread without the runtime's approved blocking mechanism. **Review, Test gate, Measure.**

Use async APIs, a bounded blocking pool, or a dedicated worker. Ensure cancellation and shutdown include that work.

### 7.2 Holding a lock across `.await`

**Avoid** retaining mutex/RwLock guards or mutable borrows across `.await`; it can deadlock, serialize unrelated work, and make cancellation leave surprising state. **Clippy gate, Review, Test gate.**

Copy/move the needed state, release the guard, await, then reacquire and revalidate. Some async mutex designs permit this, but it still requires explicit justification.

### 7.3 Unbounded task spawning

**Avoid** one spawned task per item/request with no concurrency limit. **Review, Test gate, Measure.**

Use bounded concurrency, worker pools, semaphores, or buffered streams. Define overload behavior.

### 7.4 Detached tasks and dropped join handles

**Avoid** fire-and-forget tasks whose errors, panics, lifetime, and shutdown are unowned. **Review, Test gate.**

Keep handles in a task set/supervisor, propagate cancellation, and await termination. Explicitly documented process-lifetime telemetry may be an exception.

### 7.5 Lost cancellation safety

**Avoid** placing non-cancellation-safe futures in `select!` loops or dropping a future after it partially consumes protocol state. **Review, Test gate.**

Check the exact runtime/dependency contract. Separate state changes from waits, make operations resumable, or pin and retain the future.

### 7.6 Assuming dropping a future rolls back effects

**Avoid** treating cancellation as transactional rollback. Side effects before the last poll remain. **Review, Test gate.**

Design idempotency, explicit commit points, and cleanup. Test cancellation at each await boundary where state matters.

### 7.7 Sequential awaits for independent work

**Consider** joining independent operations when latency matters; sequential awaits serialize them. **Review, Measure.**

Do not join operations that require ordering, overwhelm resources, or complicate failure semantics. Bound concurrency.

### 7.8 Async where no suspension is useful

**Avoid** async wrappers around immediate CPU-only work and async traits solely for uniformity. They enlarge state machines and spread runtime coupling. **Review, Measure.**

Keep synchronous domain logic synchronous and call it from async boundaries.

### 7.9 Unbounded channels and missing backpressure

**Avoid** unbounded queues for workload flow. Producers can outrun consumers until memory exhaustion. **Review, Test gate, Measure.**

Use bounded channels and define wait, drop, coalesce, or reject behavior. Monitor depth where operationally useful.

### 7.10 Busy polling and timer loops

**Avoid** loops that repeatedly poll `try_*`, yield, or sleep for tiny intervals. **Review, Measure.**

Await a notification, stream, or timer. Coalesce UI work where only the newest state matters.

### 7.11 Runtime-dependent behavior leaking into domain logic

**Avoid** hard-coding executor handles, clocks, spawns, and channels throughout business logic. **Review, Test gate.**

Keep domain transformations synchronous where practical and contain runtime integration at explicit boundaries. Add abstractions only where tests or multiple implementations require them.

## 8. Concurrency and synchronization

### 8.1 Shared mutable state by default

**Avoid** `Arc<Mutex<Everything>>`. It obscures ownership, creates one contention/failure domain, and invites lock-order problems. **Review, Measure.**

Partition state by invariant or assign one owner and communicate. One simple lock can still be correct and preferable at small scale.

### 8.2 Oversized critical sections

**Avoid** allocation, callbacks, rendering, parsing, I/O, logging with expensive formatting, and blocking operations while holding a lock. **Review, Measure.**

Prepare outside the lock, mutate only protected state, then release. Revalidate assumptions after reacquisition.

### 8.3 Inconsistent lock order

**Never** acquire multiple locks in inconsistent order. **Review, Test gate.**

Document one order, combine state under one lock when invariants are inseparable, or redesign ownership. Tests cannot prove deadlock absence, so review is mandatory.

### 8.4 Calling unknown code under a lock

**Avoid** callbacks, trait methods, user code, destructors, or event dispatch while holding internal locks. Re-entrancy can deadlock or observe partial state. **Review, Test gate.**

Capture work, release the lock, then invoke.

### 8.5 Incorrect atomic ordering

**Never** choose `Relaxed`, `Acquire`, `Release`, or compare-exchange orderings by intuition alone. **Review, Test gate.**

State the synchronization relation and invariant. Prefer locks when the proof is not small and clear. Use concurrency model testing where practical; ordinary tests rarely expose weak-memory failures.

### 8.6 Multiple atomics pretending to be one transaction

**Avoid** independently updating atomics when readers require a coherent multi-field snapshot. **Review, Test gate.**

Use a lock, versioned snapshot protocol, or one encoded atomic only when the representation and ordering proof are clear.

### 8.7 Assuming `RwLock` is faster

**Avoid** choosing an RwLock merely because reads are common. Writer starvation, implementation policy, and cache traffic may make it worse. **Review, Measure.**

Start with the simplest lock and benchmark representative contention.

### 8.8 Ignoring poisoning or treating it as recovery

**Avoid** unconditional poisoned-lock `unwrap` and equally avoid assuming poison proves data corruption. **Review, Test gate.**

Define whether to abort, rebuild state, inspect the inner value, or propagate failure based on invariants.

### 8.9 False sharing and hot global counters

**Consider** cache-line contention from adjacent atomics and globally updated metrics only after profiling. **Measure.**

Shard/batch counters or adjust layout when evidence supports it; padding everything wastes memory.

### 8.10 Thread lifetime and shutdown leaks

**Avoid** threads blocked forever on channels or condition variables after owners drop. **Review, Test gate.**

Define stop signals, sender ownership, wakeup, join order, and timeout policy. Test clean and failed shutdown.

### 8.11 Assuming `Send + Sync` implies semantic thread safety

`Send` and `Sync` cover Rust's data-race model, not higher-level ordering, atomicity, re-entrancy, or protocol validity. **Review, Test gate.**

Document and test semantic concurrency guarantees separately.

## 9. Unsafe Rust and FFI

### 9.1 Unsafe without necessity

**Never** introduce unsafe merely for convenience, to bypass the borrow checker, or for hypothetical speed. **Review, Measure.**

First use safe Rust or a vetted dependency. Record the requirement and evidence that justify the unsafe boundary.

### 9.2 Large unsafe blocks

**Avoid** grouping multiple unsafe operations in one block. It becomes unclear which invariant justifies which operation. **Clippy gate, Review.**

Wrap the smallest operation and place a `// SAFETY:` explanation immediately before it. The enforced lint requires unsafe blocks to be documented and discourages multiple unsafe operations per block.

### 9.3 Safety comments that restate syntax

**Never** accept “pointer is valid” as sufficient proof. **Review.**

Explain provenance, initialization, alignment, bounds, aliasing, lifetime, thread, and ownership facts as applicable, and identify who establishes them.

### 9.4 Unsound `unsafe impl Send/Sync`

**Never** add unsafe auto-trait implementations without proving every field and reachable operation satisfies cross-thread requirements. **Review, Test gate.**

Prefer designs whose auto traits are derived. Negative/non-implementation may be the correct contract.

### 9.5 `transmute` as conversion

**Avoid** `transmute` for ordinary casts, enum decoding, lifetime extension, or layout conversion. Size equality is not validity. **Review, Test gate.**

Use explicit conversions, byte APIs, `TryFrom`, pointer methods, or well-reviewed crates. Never manufacture a longer lifetime.

### 9.6 Uninitialized or partially initialized memory mistakes

**Never** create invalid values with `mem::uninitialized`, `zeroed`, or incorrect `MaybeUninit` usage. **Compiler, Review, Test gate.**

Track exactly which elements are initialized and how partial failure drops them. Zero is not valid for references, many enums, and numerous library types.

### 9.7 Raw slice construction without complete proof

**Never** call `from_raw_parts`/`from_raw_parts_mut` without proving non-nullness (including zero length where required by the API), alignment, one-allocation bounds, initialization, aliasing, and lifetime. **Review.**

Keep the unsafe constructor private behind a safe API whose inputs establish those facts.

### 9.8 Violating aliasing through raw pointers or interior mutation

**Never** create overlapping mutable references, mutate through shared references without proper interior-mutability primitives, or retain references across relocation/deallocation. **Review, Test gate.**

Raw pointers do not erase aliasing obligations when converted back to references.

### 9.9 Depending on unspecified layout

**Never** expose or reinterpret default Rust layout across FFI, persistence, hashing, or byte protocols. **Review, Test gate.**

Use explicit serialization or the appropriate `repr(C)`/transparent representation, and still validate field validity and ABI details. `repr(C)` does not make arbitrary Rust types FFI-safe.

### 9.10 FFI ownership ambiguity

**Never** leave allocation/freeing side, pointer lifetime, nullability, thread affinity, callback lifetime, or error ownership implicit. **Review, Test gate.**

Define paired constructors/destructors and convert to safe owned types at one narrow boundary. Do not free memory with a different allocator.

### 9.11 C strings and foreign buffers

**Never** assume foreign strings are non-null, NUL-terminated, UTF-8, immutable, or alive long enough. **Review, Test gate.**

Validate the exact contract, use `CStr`/`CString` correctly, and copy when lifetime cannot be guaranteed.

### 9.12 Unwinding through foreign code

**Never** unwind through an incompatible ABI. **Review, Test gate.**

Use the correct ABI and panic policy, catch at the boundary when supported, and convert to an explicit foreign error result.

### 9.13 Pinning misconceptions

**Avoid** `Pin`, `Unpin` manipulation, and projection code without understanding structural pinning and drop guarantees. **Review.**

Use established projection tools when requirements justify pinning. Pinning does not by itself make self-references safe.

## 10. I/O, parsing, and serialization

### 10.1 Assuming one read or write completes the operation

**Avoid** treating `read`/`write` as full-buffer operations. Partial progress is valid. **Review, Test gate.**

Use `read_exact`, `write_all`, buffered adapters, or explicit loops according to protocol semantics. Preserve partial-progress errors where relevant.

### 10.2 Unbounded reads and allocations

**Never** read arbitrary files, streams, archive entries, image dimensions, or declared lengths into memory without approved bounds. **Review, Test gate.**

Stream, cap, validate before allocation, and defend against integer overflow and decompression bombs.

### 10.3 Ignoring flush, sync, rename, and close semantics

**Avoid** claiming persistence or atomic replacement without defining buffering, `flush`, durability, temporary-file, rename, and directory-sync requirements for the target platform. **Review, Test gate.**

Implement only the durability level the product requires and document it. A successful write is not automatically durable storage.

### 10.4 Path-to-string conversion

**Avoid** assuming paths are UTF-8 or converting with `to_string_lossy` for identity/security decisions. **Review, Test gate.**

Keep `Path`/`OsStr` internally. Convert for display only with an explicit lossy policy.

### 10.5 Path traversal and symlink races

**Never** trust joined user paths, lexical prefixes, or a check-then-open sequence as a complete containment guarantee. **Review, Test gate.**

Define the threat model and use platform-appropriate safe-open/canonicalization strategies. Be explicit about symlink policy.

### 10.6 Ad hoc parsers that accept ambiguous input

**Avoid** split-based parsing for escaping, quoting, nesting, Unicode, or versioned formats. **Review, Test gate.**

Use a defined grammar or vetted parser. Reject trailing/duplicate/unknown data when required; preserve it when forward compatibility requires that instead.

### 10.7 Serialization as a stable format by accident

**Avoid** persisting derived in-memory layout with no schema/version/migration policy. Field or variant changes can break data silently. **Review, Test gate.**

Define a stable external schema, versions, defaults, unknown-field behavior, and migration tests.

### 10.8 Deserializing directly into trusted domain state

**Avoid** assuming successful syntax decoding establishes domain validity. **Review, Test gate.**

Deserialize into boundary types, validate sizes and cross-field invariants, then construct domain types.

### 10.9 Reading text when bytes are the contract

**Avoid** forcing UTF-8 onto binary protocols and treating line APIs as lossless for arbitrary files. **Review, Test gate.**

Use byte-oriented APIs and decode only where encoding is specified.

### 10.10 Per-item synchronous I/O

**Consider** batching, buffering, or streaming when profiles show many tiny reads/writes or metadata calls. **Measure.**

Do not add buffering layers blindly; they affect latency, flush semantics, and memory.

## 11. Macros, build configuration, and dependencies

### 11.1 Macros where functions or traits suffice

**Avoid** declarative/procedural macros for ordinary reuse. They increase diagnostic, tooling, hygiene, and compile-time complexity. **Review.**

Use a function, generic, or trait unless syntax generation or variadic structure is genuinely required.

### 11.2 Macros with hidden control flow or evaluation

**Never** write macros that evaluate an argument multiple times unexpectedly, silently return/break, create surprising identifiers, or hide fallible work. **Review, Test gate.**

Evaluate expressions once, use hygienic paths, document control flow, and test expansion-relevant edge cases.

### 11.3 Giant generated code checked in without boundaries

**Avoid** mixing generated and authored code or requiring humans to review generated output as ordinary source. **Review.**

Separate, mark, reproduce, and inventory generated files. Pin the generator and verify regeneration when needed.

### 11.4 Feature combinations that are not additive

**Avoid** Cargo features that disable behavior, are mutually exclusive without validation, or compile only in the default combination. **Review, Test gate.**

Features should generally be additive. Test supported combinations and reject invalid combinations with clear compile errors.

### 11.5 Platform behavior hidden behind scattered `cfg`

**Avoid** widespread conditional compilation that creates unreviewed alternate programs. **Review, Test gate.**

Contain platform differences behind small modules and compile/test every supported target. Reject unsupported targets explicitly when appropriate.

### 11.6 Build scripts doing uncontrolled work

**Avoid** network access, nondeterministic generation, undeclared environment dependence, broad filesystem scans, and missing rerun directives in `build.rs`. **Review, Test gate.**

Keep builds hermetic, deterministic, narrowly invalidated, and failure-explicit.

### 11.7 Dependency for trivial functionality

**Avoid** adding a crate without comparing maintenance, transitive graph, features, platform support, unsafe surface, license, and failure modes against a small local implementation. **Review.**

Also avoid reimplementing security-sensitive or complex standards that a vetted dependency handles better.

### 11.8 Unpinned tools and accidental dependency features

**Avoid** floating CI tools, broad default features, and versions chosen without compatibility evidence. **Review, Test gate.**

Pin repository tools, select needed crate features, inspect the resolved graph, and commit lockfiles according to repository policy.

### 11.9 Multiple crates and workspaces without a deployment boundary

**Avoid** splitting into crates solely for visual organization. It increases manifests, versioning, compile boundaries, and dependency plumbing. **Review, Measure.**

Create a crate for a meaningful reuse, isolation, platform, build, or ownership boundary.

### 11.10 Abusing `include!`, environment variables, and compile-time embedding

**Avoid** hidden source inclusion, absolute build paths, large embedded assets, and compile-time environment behavior without reproducibility needs. **Review, Test gate, Measure.**

Use explicit modules/resources and define invalidation and deployment behavior.

## 12. Testing and validation anti-patterns

### 12.1 Testing implementation details instead of behavior

**Avoid** tests coupled to private call order, formatting, incidental allocation, or internal type layout. They block refactoring without protecting requirements. **Review.**

Assert observable outcomes, state transitions, and contracts. Test internal algorithms directly only when they have a meaningful independent contract.

### 12.2 Happy-path-only testing

**Never** omit approved edge, boundary, cancellation, malformed-input, and dependency-failure behavior. **Test gate, Review.**

Map each acceptance criterion and documented failure mode to tests before product code.

### 12.3 Tests that cannot fail for the intended reason

**Avoid** assertions that merely repeat setup, snapshots no one inspects, and mocks that implement the same bug as production. **Review, Mutation gate.**

Observe the test fail before implementation and use mutation testing for affected product logic.

### 12.4 Sleeping and timing races

**Avoid** fixed sleeps, tight timeouts, and “eventually” loops with no diagnostic state. **Review, Test gate.**

Use notifications, injected clocks, deterministic executors, and bounded waits that report the unmet condition.

### 12.5 Shared global test state

**Avoid** environment variables, current-directory changes, fixed ports, fixed temp paths, and process globals that make tests order-dependent or nonparallel. **Review, Test gate.**

Inject configuration, use unique resources, and restore unavoidable process state safely.

### 12.6 Over-mocking

**Avoid** mocking every collaborator and asserting interactions rather than results. Tests become an alternate implementation. **Review.**

Use real in-memory components and fakes at actual nondeterministic or external boundaries.

### 12.7 Property tests without meaningful generators and oracles

**Avoid** random tests that mostly generate invalid/trivial data or only assert “does not panic.” **Review.**

Generate domain-valid and boundary-heavy cases, state invariants, shrink failures, and fix seeds/reproduction data.

### 12.8 Snapshot sprawl

**Avoid** large snapshots that reviewers update blindly or that contain nondeterministic data. **Review.**

Snapshot stable, human-reviewable outputs; assert critical semantics separately.

### 12.9 Ignoring platform and feature matrices

**Avoid** claiming support for configurations never compiled or tested. **Test gate, Review.**

Run each supported platform/toolchain/feature combination at an appropriate cadence and state exclusions explicitly.

### 12.10 Coverage as proof of correctness

100% line coverage proves execution, not correct assertions, branch completeness, race safety, or requirement fit. **Review, Mutation gate.**

Combine coverage with requirement-based tests, boundary cases, mutation testing, and code review.

## 13. Logging, diagnostics, and observability

### 13.1 Logging secrets or unbounded user data

**Never** log credentials, tokens, private document content, or arbitrary payloads without an approved redaction policy. **Review, Test gate.**

Log stable identifiers and bounded metadata needed for diagnosis.

### 13.2 Logging in hot loops

**Avoid** per-item or per-frame logs, especially with eager formatting, unless explicitly sampled or enabled for diagnosis. **Review, Measure.**

Use levels, structured fields, aggregation, and lazy formatting supported by the logging API.

### 13.3 Logs as control flow or durable state

**Never** rely on a log message to make behavior correct, synchronize components, or persist required user state. **Review.**

Use typed state and explicit protocols.

### 13.4 Vague diagnostics

**Avoid** “operation failed” with no operation, safe identifier, source error, or next action. **Review, Test gate.**

Record enough context to locate the transition while avoiding sensitive data and duplication.

### 13.5 Speculative telemetry

**Avoid** adding logs/metrics “just in case.” They impose noise, privacy, storage, and maintenance costs. **Review.**

Tie diagnostics to approved failure paths and operational decisions.

## 14. Maintainability and productivity

### 14.1 Clever code over obvious code

**Avoid** compressed expressions, type tricks, macro metaprogramming, and iterator puzzles that save lines but increase proof effort. **Review.**

Prefer direct control flow and names that expose intent. Concision is useful only while clarity remains.

### 14.2 Premature abstraction

**Avoid** factories, registries, plugin layers, repositories, strategy traits, and generic frameworks for hypothetical requirements. **Review.**

Implement the current behavior directly, then extract when repeated change demonstrates a stable boundary.

### 14.3 Copy-paste drift

**Avoid** duplicated domain rules and bug fixes across branches/modules. **Review, Test gate.**

Extract the smallest shared rule once duplication demonstrates shared semantics. Do not deduplicate coincidentally similar code with different reasons to change.

### 14.4 Functions with mixed responsibilities

**Avoid** functions that parse, validate, mutate, perform I/O, render, log, and recover in one flow. **Review.**

Split at state transitions and fallible boundaries. Do not split into one-line wrappers that obscure navigation.

### 14.5 Generic “utils”, “helpers”, and “manager” modules

**Avoid** dumping unrelated behavior into weakly named modules/types. **Review.**

Place operations with the domain concept they serve and name them by responsibility.

### 14.6 Comments that narrate syntax or preserve dead history

**Avoid** comments that repeat code, commented-out code, TODOs without approved tracking, and historical explanations available from version control. **Review.**

Document why, invariants, safety, non-obvious constraints, and externally imposed behavior. Keep comments synchronized with code.

### 14.7 Names that hide units, ownership, or effects

**Avoid** vague names (`data`, `thing`, `handle`, `process`) and methods that look like accessors but perform I/O or mutation. **Review.**

Name domain meaning and important effects. Do not encode every type detail into names.

### 14.8 Modules exposing internal representation

**Avoid** callers constructing internal nodes, mutating indexes, or depending on storage layout. **Review.**

Expose a cohesive API that preserves invariants, while avoiding needless getter boilerplate.

### 14.9 Configuration bags passed everywhere

**Avoid** one giant context/config object that gives every function access to unrelated services and settings. **Review.**

Pass required dependencies explicitly or group genuinely cohesive capabilities. Avoid a service locator.

### 14.10 Global mutable state and hidden singletons

**Avoid** process-wide mutable state for application data, caches, runtime handles, and test configuration. **Review, Test gate.**

Use explicit ownership and scoped initialization. Immutable constants and carefully bounded one-time initialization are appropriate.

### 14.11 Layering that only forwards calls

**Avoid** controller/service/repository wrappers with no policy, translation, or isolation. **Review.**

Remove the layer or give it a concrete responsibility justified by requirements.

### 14.12 Domain logic coupled to UI/runtime frameworks

**Avoid** embedding GPUI entities, views, executors, or event plumbing into calculations and state transitions that can remain ordinary Rust. **Review, Test gate.**

Keep domain logic independent where practical, but introduce an abstraction only when a concrete requirement justifies it.

### 14.13 Broad refactors mixed with behavior changes

**Avoid** formatting, renaming, dependency updates, and architectural cleanup in the same change as one behavior. **Review.**

Keep diffs independently deployable and small enough for complete review.

### 14.14 Warnings and lint exceptions treated as cleanup debt

**Never** merge new warnings or blanket suppression with a promise to fix later. **Compiler, Clippy gate, Review.**

Fix the cause or add one narrow, reasoned expectation approved with the change.

## 15. Security and robustness traps

### 15.1 Trusting external lengths, indexes, and counts

**Never** allocate, slice, loop, or recurse directly from unvalidated external values. **Review, Test gate.**

Validate bounds, conversion, multiplication/addition overflow, and remaining input before use.

### 15.2 Unsafe temporary-file patterns

**Avoid** predictable names, check-then-create, permissive permissions, and non-atomic replacement for sensitive data. **Review, Test gate.**

Use secure creation APIs and define cleanup and replacement semantics.

### 15.3 Comparing secrets with ordinary equality

**Avoid** ordinary early-exit comparison when timing exposure is in the threat model. **Review.**

Use vetted constant-time primitives; do not hand-roll cryptography.

### 15.4 Command and shell construction

**Never** concatenate untrusted strings into a shell command. **Review, Test gate.**

Invoke programs with separate arguments and validate the executable/path policy. Avoid a shell unless shell syntax is the explicit requirement.

### 15.5 Integer truncation at OS/FFI boundaries

**Never** assume `usize`, descriptor, timestamp, offset, or count widths match a foreign type. **Review, Test gate.**

Use checked conversions and handle platform-specific limits.

### 15.6 Resource exhaustion omitted from the model

**Avoid** assuming allocation, thread/task creation, file descriptors, queues, recursion, and caches are effectively unlimited. **Review, Test gate, Measure.**

Set limits and define refusal/degradation behavior for externally driven growth.

## Agent and reviewer checklist

For every Rust change, record applicable findings rather than merely stating “looks good.”

### Correctness

- [ ] Every approved state transition and dependency failure has explicit behavior.
- [ ] No meaningful `Result`, task/thread join, write, flush, or cleanup failure is silently discarded.
- [ ] Panics, indexing, unwraps, expects, casts, arithmetic, floats, Unicode, and time assumptions are justified and tested.
- [ ] Invalid states and units are represented explicitly.
- [ ] Partial mutation, cancellation, retry, and shutdown preserve invariants.

### Ownership and API

- [ ] Each clone, allocation, `Rc`/`Arc`, interior-mutability primitive, box, and `'static` bound has a concrete ownership reason.
- [ ] Borrows are no broader or longer than needed, especially across callbacks, I/O, locks, and awaits.
- [ ] Public APIs are minimal, typed, unsurprising, and preserve invariants.
- [ ] Traits, generics, builders, and abstractions solve current requirements rather than hypothetical reuse.

### Performance

- [ ] No performance claim relies on intuition alone.
- [ ] Collection and algorithm complexity fit expected input sizes.
- [ ] Hot-path allocation, cloning, formatting, dispatch, locking, and I/O were measured when relevant.
- [ ] Benchmarks use the intended profile, platform, and representative workloads and report reproducible results.
- [ ] Optimizations do not weaken correctness, soundness, readability, or required portability.

### Async and concurrency

- [ ] Blocking work, spawned work, channels, and queues are bounded and owned.
- [ ] No lock or guard unintentionally crosses `.await`, callbacks, or expensive work.
- [ ] Cancellation safety, backpressure, task errors, lock order, shutdown, and retry are defined and tested.
- [ ] Atomic ordering and unsafe `Send`/`Sync` claims have explicit proofs.

### Unsafe and boundaries

- [ ] Unsafe code is necessary, minimal, and immediately documented with complete invariants.
- [ ] FFI layout, validity, lifetime, ownership, allocator, nullability, threading, and panic behavior are explicit.
- [ ] External lengths, paths, text, serialized values, and protocol data are bounded and validated before entering domain state.

### Tests and maintainability

- [ ] Tests map to acceptance criteria, edge cases, and documented dependency failures and were observed failing first.
- [ ] Tests are deterministic, isolated, behavior-focused, and meaningful under mutation testing.
- [ ] Logs are approved, safe, bounded, and emitted at the layer owning the outcome.
- [ ] The diff contains no unrelated refactor, speculative feature, dependency, suppression, generated file, or framework.
- [ ] Rustfmt, the registered quality target, coverage, and applicable mutation groups pass.

## Primary references

These primary sources define language/tool behavior; this document adds repository policy. Links were checked on 2026-09-25. The repository pins Rust 1.98.1 and Clippy 0.1.98; use documentation matching the pinned toolchain when lint behavior differs.

- [The Rust Reference](https://doc.rust-lang.org/reference/)
- [The Rust standard library documentation](https://doc.rust-lang.org/1.98.0/std/)
- [The Rustonomicon](https://doc.rust-lang.org/nomicon/)
- [The Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Clippy lint documentation for Rust 1.98.0](https://rust-lang.github.io/rust-clippy/rust-1.98.0/index.html)
- [Clippy lint groups](https://doc.rust-lang.org/clippy/lints.html)
- [Cargo Reference](https://doc.rust-lang.org/cargo/reference/)
- [Async Book](https://rust-lang.github.io/async-book/)
- [Tokio: Bridging with synchronous code](https://tokio.rs/tokio/topics/bridging)
- [Tokio `select!` cancellation safety](https://docs.rs/tokio/1/tokio/macro.select.html#cancellation-safety)
- [Rust Unsafe Code Guidelines Reference](https://rust-lang.github.io/unsafe-code-guidelines/)
