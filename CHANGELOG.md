# Changelog

## [0.7.2](https://github.com/seuros/escrow_machines/compare/escrow_machines/v0.7.1...escrow_machines/v0.7.2) (2026-11-16)


### Bug Fixes

* fix wrapped classes at load so building a value runs no Ruby ([3e0238a](https://github.com/seuros/escrow_machines/commit/3e0238a43954f3fe90537ac42df5c48cd77868c5))

## [0.7.1](https://github.com/seuros/escrow_machines/compare/escrow_machines/v0.7.0...escrow_machines/v0.7.1) (2026-11-16)


### Bug Fixes

* refuse merges that disagree on a transfer's cumulative total ([cbcafa5](https://github.com/seuros/escrow_machines/commit/cbcafa54cbb866a9f5c425dbcbbad90b09694f6e))

## [0.7.0](https://github.com/seuros/escrow_machines/compare/escrow_machines/v0.6.0...escrow_machines/v0.7.0) (2026-11-16)


### ⚠ BREAKING CHANGES

* fold FrameError into Refusal, new 0xE6 frame of plain tallies

### Features

* fold FrameError into Refusal, new 0xE6 frame of plain tallies ([04c0d32](https://github.com/seuros/escrow_machines/commit/04c0d32b6be44d1fdff8a3cec23cd39fbbf54233))

## [0.6.0](https://github.com/seuros/escrow_machines/compare/escrow_machines/v0.5.0...escrow_machines/v0.6.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* escrow partner refusals name the class received

### Features

* escrow partner refusals name the class received ([0efa5da](https://github.com/seuros/escrow_machines/commit/0efa5dae455c8a2b571983024753116c57f3c89f))

## [0.5.0](https://github.com/seuros/escrow_machines/compare/escrow_machines/v0.4.0...escrow_machines/v0.5.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* adopt matryona 0.3: decode takes exactly a String, names_only refusals

### Features

* adopt matryona 0.3: decode takes exactly a String, names_only refusals ([73f1174](https://github.com/seuros/escrow_machines/commit/73f1174ac829f6b089626409483be444de4ae562))

## [0.4.0](https://github.com/seuros/escrow_machines/compare/escrow_machines/v0.3.0...escrow_machines/v0.4.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* adopt matryona-machines 0.2: exact Receipt fields, exact-class Escrow partners, honoured replays proven unchanged

### Features

* adopt matryona-machines 0.2: exact Receipt fields, exact-class Escrow partners, honoured replays proven unchanged ([78045e6](https://github.com/seuros/escrow_machines/commit/78045e6f0722a55c2804920207ae197a1e472ac9))

## [0.3.0](https://github.com/seuros/escrow_machines/compare/escrow_machines/v0.2.0...escrow_machines/v0.3.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* private Mark/Receipt fields, sealed validated values, clone-then-keep transfer, compile-time frame bound

### Features

* private Mark/Receipt fields, sealed validated values, clone-then-keep transfer, compile-time frame bound ([223b8cb](https://github.com/seuros/escrow_machines/commit/223b8cb5dcb34e5cb615c8f7667b17bb7eac8e16))

## [0.2.0](https://github.com/seuros/escrow_machines/compare/escrow_machines/v0.1.1...escrow_machines/v0.2.0) (2026-11-13)


### ⚠ BREAKING CHANGES

* see CONVENTIONS: one exhaustive Refusal per crate, Result from every mutator, honoured idempotent repeats, Ruby mirrors the core. No compatibility aliases. replica/dump/load are local/encode/decode; transfer answers Transfer::Applied or Transfer::Replayed.

### Features

* Ruby uses the core's names and positional arguments, Transfer variants as values ([8d83f20](https://github.com/seuros/escrow_machines/commit/8d83f20731ca496ebfb251480f9895e7726b3367))

## [0.1.1](https://github.com/seuros/escrow_machines/compare/escrow_machines-v0.1.0...escrow_machines/v0.1.1) (2026-11-12)


### Features

* initial extraction ([d0edc22](https://github.com/seuros/escrow_machines/commit/d0edc223cc69da6ca9b35d07940545a439464356))
