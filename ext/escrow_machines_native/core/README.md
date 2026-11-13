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

The counters are `merge-machines` `GCounter`s and `Escrow` implements its
`Merge` trait. The escrow adds what a PN-counter lacks: writes are restricted to
the local replica's own cells and bounded by its balance.

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

The Ruby gem mirrors `Receipt` and `Mark` as `Data` values of the same names,
and `Transfer` as one `Data` per variant, `Transfer::Applied` and
`Transfer::Replayed`, each carrying the `receipt`.

Initial, unstable API. No clock, I/O, hashing, or randomness.
