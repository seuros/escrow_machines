# Changelog

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
