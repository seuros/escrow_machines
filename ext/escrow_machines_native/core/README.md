# escrow-machines

> **AMS O'Neil**, NCC-4233, *O'Neil*-class treasury escort.

Engine-room flight manual: move counted rights between stations and never spend what is not in the hold.

Allocation-free `no_std` bounded-counter CRDT. A fixed set of up to `N` replicas
shares a fixed supply of rights. `Escrow<N>` is one replica's view: a matrix of
cumulative grants between replicas plus per-replica spent and refunded counters.
`spend` and `transfer` are checked against the rights the local replica holds,
so the total spent cannot exceed the supply under any partition. `refund`
returns only what the local replica itself spent. Nothing is minted after
construction.

`merge` is a cell-wise join: commutative, associative, idempotent. `transfer`
takes a caller-chosen id, strictly increasing per `(from, to)` pair; repeating
the latest id returns the original `Receipt` and moves nothing. Receipts carry
the cumulative grant, so `accept` is idempotent and order-insensitive. Every
refusal is a `Refusal` value and leaves the state untouched; counters refuse at
`u64::MAX` instead of wrapping.

`Escrow` implements the `merge-machines` `Merge` trait. Replica ids are
dense, `0..replicas` with `replicas <= N`, so every per-replica table is a
plain `[T; N]` read through a position that is below `N` by construction: a
lookup cannot miss. The counters are such tables, one `u64` cell per replica
joined by `max`, so raising one never needs a free slot and its only refusal
is `Overflow`. Frame refusals are `Refusal` variants like every other
(`Truncated`, `WrongStructure`, `WrongShape`, `Malformed`); there is no second
error type. The frame stores each counter as `N` 64-bit fields; it leads with
`0xE6`, and a frame from before (`0xE5`, which embedded `merge-machines`
counter frames) is `WrongStructure`.

The escrow adds what a PN-counter lacks: writes are restricted to the local
replica's own cells and bounded by its balance.

Contracts the caller owns:

- **Durable replica identity.** Persist `encode`'s fixed-length frame before
  acting on a successful call. A replica restarted from an older frame can
  double-spend; `merge` answers `Diverged` once a peer proves it, and `recover`
  adopts the peer's record explicitly (`Overdrawn` if the histories cannot both
  hold). Spends no peer saw are unrecoverable.
- **Transfer ids.** Monotonic per destination, reused on retry. Only the latest
  id per pair replays; older ids answer `StaleId`.
- **Authenticity and transport.** Contradictory receipts and frames are refused;
  fabricated but self-consistent ones are not detectable here.
- **Membership.** The replica set and allotment are fixed at construction and
  must be identical on every replica.

`Receipt::new` and `Mark::new` build only values some call could produce
(`None` for a receipt naming one replica twice, moving zero or more than its
`total`, and for a mark of zero). Their fields are private, read through
`from()`, `to()`, `id()`, `amount()`, `total()`, so no receipt contradicts
itself and `accept` needs no check for one: its `Malformed` is a receipt that
contradicts the grants already seen.

No call takes a clock, so no answer records one.

The Ruby gem mirrors `Receipt` and `Mark` as `Data` values of the same names,
validated through those constructors, and `Transfer` as one `Data` per
variant, `Transfer::Applied` and `Transfer::Replayed`, each carrying the
`receipt`.

Pre-1.0: breaking changes land in minor versions. No clock, I/O, hashing, or randomness.
