use super::*;

mod edges;

type Ship = Escrow<4>;

/// A receipt the test knows is consistent with itself.
fn receipt(from: ReplicaId, to: ReplicaId, id: u64, amount: u64, total: u64) -> Receipt {
    Receipt::new(from, to, id, amount, total).expect("a receipt consistent with itself")
}

/// Caller-supplied draws for the randomized runs.
struct Draws(u64);

impl Draws {
    const fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
    fn pick(&mut self, bound: usize) -> usize {
        usize::try_from(self.below(bound as u64)).expect("small")
    }
}

/// One transfer that happened, in the order it happened.
struct Event {
    from: usize,
    to: usize,
    id: u64,
    amount: u64,
}

/// The naive reference: a global log of transfers and, per replica, the set
/// of log entries it has heard of. Merging is set union.
struct Model {
    allotted: Vec<u64>,
    events: Vec<Event>,
    known: Vec<Vec<bool>>,
    net_spent: Vec<u64>,
}

impl Model {
    fn new(allotted: &[u64]) -> Self {
        Self {
            allotted: allotted.to_vec(),
            events: Vec::new(),
            known: vec![Vec::new(); allotted.len()],
            net_spent: vec![0; allotted.len()],
        }
    }
    fn balance(&self, replica: usize) -> u64 {
        let mut balance = i128::from(self.allotted[replica]) - i128::from(self.net_spent[replica]);
        for (event, known) in self.events.iter().zip(&self.known[replica]) {
            if event.from == replica {
                balance -= i128::from(event.amount);
            } else if event.to == replica && *known {
                balance += i128::from(event.amount);
            }
        }
        u64::try_from(balance).expect("a replica never goes below zero")
    }
    fn last(&self, from: usize, to: usize) -> Option<usize> {
        self.events
            .iter()
            .rposition(|event| event.from == from && event.to == to)
    }
    fn total(&self, upto: usize) -> u64 {
        let last = &self.events[upto];
        self.events[..=upto]
            .iter()
            .filter(|event| event.from == last.from && event.to == last.to)
            .map(|event| event.amount)
            .sum()
    }
    fn receipt(&self, index: usize) -> Receipt {
        let event = &self.events[index];
        receipt(
            event.from as ReplicaId,
            event.to as ReplicaId,
            event.id,
            event.amount,
            self.total(index),
        )
    }
    fn transfer(
        &mut self,
        from: usize,
        to: usize,
        id: u64,
        amount: u64,
    ) -> Result<Transfer, Refusal> {
        if amount == 0 {
            return Err(Refusal::ZeroAmount);
        }
        if to >= self.allotted.len() {
            return Err(Refusal::UnknownReplica);
        }
        if from == to {
            return Err(Refusal::SelfTransfer);
        }
        if let Some(last) = self.last(from, to) {
            let event = &self.events[last];
            if id < event.id {
                return Err(Refusal::StaleId { latest: event.id });
            }
            if id == event.id && amount != event.amount {
                return Err(Refusal::IdConflict {
                    amount: event.amount,
                });
            }
            if id == event.id {
                return Ok(Transfer::Replayed(self.receipt(last)));
            }
        }
        let available = self.balance(from);
        if amount > available {
            return Err(Refusal::Insufficient { available });
        }
        self.events.push(Event {
            from,
            to,
            id,
            amount,
        });
        for (replica, known) in self.known.iter_mut().enumerate() {
            known.push(replica == from);
        }
        Ok(Transfer::Applied(self.receipt(self.events.len() - 1)))
    }
    fn accept(&mut self, replica: usize, index: usize) -> Accepted {
        if self.known[replica][index] {
            return Accepted::Replayed;
        }
        let (from, to) = (self.events[index].from, self.events[index].to);
        for earlier in 0..=index {
            if self.events[earlier].from == from && self.events[earlier].to == to {
                self.known[replica][earlier] = true;
            }
        }
        Accepted::Applied
    }
    fn merge(&mut self, into: usize, other: usize) {
        let theirs = self.known[other].clone();
        for (mine, theirs) in self.known[into].iter_mut().zip(theirs) {
            *mine |= theirs;
        }
    }
    fn in_flight(&self) -> u64 {
        let unseen = |(index, event): (usize, &Event)| {
            (!self.known[event.to][index]).then_some(event.amount)
        };
        self.events.iter().enumerate().filter_map(unseen).sum()
    }
}

/// Every cell of a view, leaving out whose view it is.
fn cells(ship: &Ship) -> Vec<Option<u64>> {
    let mut cells = Vec::new();
    for from in 0..4 {
        cells.push(ship.spent(from));
        cells.push(ship.refunded(from));
        for to in 0..4 {
            cells.push(ship.granted(from, to));
            cells.push(ship.last_transfer(from, to).map(|mark| mark.id));
            cells.push(ship.last_transfer(from, to).map(|mark| mark.amount));
        }
    }
    cells
}

/// Rights the giver has let go of and the receiver has not heard of.
fn in_flight(fleet: &[Ship]) -> u128 {
    let mut total = 0;
    for (from, giver) in fleet.iter().enumerate() {
        for (to, receiver) in fleet.iter().enumerate() {
            let (from, to) = (from as ReplicaId, to as ReplicaId);
            let sent = giver.granted(from, to).expect("known");
            let seen = receiver.granted(from, to).expect("known");
            assert!(seen <= sent, "a receiver never sees more than was sent");
            total += u128::from(sent - seen);
        }
    }
    total
}

fn held(fleet: &[Ship]) -> u128 {
    fleet
        .iter()
        .map(|ship| {
            u128::from(ship.available())
                + u128::from(ship.outstanding(ship.local()).expect("local"))
        })
        .sum()
}

fn check(fleet: &[Ship], model: &Model, supply: u64) {
    for (replica, ship) in fleet.iter().enumerate() {
        assert_eq!(ship.available(), model.balance(replica));
        assert_eq!(
            ship.outstanding(replica as ReplicaId),
            Some(model.net_spent[replica])
        );
        let ids = 0..fleet.len() as ReplicaId;
        let view: i128 = ids
            .map(|id| {
                ship.balance(id).expect("known") + i128::from(ship.outstanding(id).expect("known"))
            })
            .sum();
        assert_eq!(view, i128::from(supply));
        assert_eq!(ship.supply(), supply);
    }
    // Conservation: every right is held, spent, or in flight. None is minted.
    assert_eq!(in_flight(fleet), u128::from(model.in_flight()));
    assert_eq!(held(fleet) + in_flight(fleet), u128::from(supply));
    let outstanding: u128 = model.net_spent.iter().map(|s| u128::from(*s)).sum();
    assert!(outstanding <= u128::from(supply));
}

fn run(seed: u64, allotted: &[u64], steps: usize) -> Vec<Ship> {
    let mut draws = Draws(seed);
    let replicas = allotted.len();
    let supply: u64 = allotted.iter().sum();
    let mut model = Model::new(allotted);
    let mut fleet: Vec<Ship> = (0..replicas)
        .map(|local| Ship::new(local as ReplicaId, allotted).expect("config"))
        .collect();
    let mut next_id = vec![vec![1_u64; 5]; replicas];
    for step in 0..steps {
        let replica = draws.pick(replicas);
        let before = fleet[replica];
        let amount = draws.below(supply / 2 + 2);
        match draws.below(8) {
            0 | 1 => {
                let available = model.balance(replica);
                let expected = if amount == 0 {
                    Err(Refusal::ZeroAmount)
                } else if amount > available {
                    Err(Refusal::Insufficient { available })
                } else {
                    model.net_spent[replica] += amount;
                    Ok(available - amount)
                };
                assert_eq!(fleet[replica].spend(amount), expected);
            }
            2 => {
                let outstanding = model.net_spent[replica];
                let expected = if amount == 0 {
                    Err(Refusal::ZeroAmount)
                } else if amount > outstanding {
                    Err(Refusal::ExceedsSpent { outstanding })
                } else {
                    model.net_spent[replica] -= amount;
                    Ok(model.balance(replica))
                };
                assert_eq!(fleet[replica].refund(amount), expected);
            }
            3 | 4 => {
                // One slot past the replicas, so unknown ones are drawn too.
                let to = draws.pick(replicas + 1);
                let fresh = next_id[replica][to];
                let id = match draws.below(4) {
                    0 => fresh.saturating_sub(1),
                    1 => fresh.saturating_sub(2),
                    _ => fresh,
                };
                let expected = model.transfer(replica, to, id, amount);
                if matches!(expected, Ok(Transfer::Applied(_))) {
                    next_id[replica][to] = id + 1 + draws.below(3);
                }
                assert_eq!(
                    fleet[replica].transfer(to as ReplicaId, id, amount),
                    expected
                );
            }
            5 if !model.events.is_empty() => {
                let index = draws.pick(model.events.len());
                let receipt = model.receipt(index);
                let expected = model.accept(replica, index);
                assert_eq!(fleet[replica].accept(&receipt), Ok(expected));
            }
            _ => {
                let other = draws.pick(replicas);
                let theirs = fleet[other];
                model.merge(replica, other);
                assert_eq!(fleet[replica].merge(&theirs), Ok(()));
            }
        }
        if fleet[replica] != before {
            assert_ne!(cells(&fleet[replica]), cells(&before));
        }
        check(&fleet, &model, supply);
        if step % 16 == 0 {
            let mut frame = [0; Ship::ENCODED_LEN];
            assert_eq!(fleet[replica].encode(&mut frame), Ok(Ship::ENCODED_LEN));
            assert_eq!(Ship::decode(&frame), Ok(fleet[replica]));
        }
    }
    // Full gossip: nothing stays in flight and every view agrees.
    for _ in 0..2 {
        for into in 0..replicas {
            for other in 0..replicas {
                let theirs = fleet[other];
                model.merge(into, other);
                assert_eq!(fleet[into].merge(&theirs), Ok(()));
            }
        }
    }
    check(&fleet, &model, supply);
    assert_eq!(in_flight(&fleet), 0);
    assert_eq!(held(&fleet), u128::from(supply));
    for ship in &fleet {
        assert_eq!(cells(ship), cells(&fleet[0]));
        for (replica, peer) in fleet.iter().enumerate() {
            assert_eq!(
                ship.balance(replica as ReplicaId),
                Some(i128::from(peer.available()))
            );
        }
    }
    fleet
}

#[test]
fn randomized_runs_match_the_naive_model_and_conserve_rights() {
    for seed in 1..=40_u64 {
        run(
            seed.wrapping_mul(0x9E37_79B9_7F4A_7C15),
            &[50, 30, 0, 20],
            600,
        );
        run(seed.wrapping_mul(0xD1B5_4A32_D192_ED03), &[7, 0, 9], 400);
        run(seed, &[3, 3], 200);
    }
}

#[test]
fn refused_calls_in_a_randomized_run_leave_the_state_untouched() {
    let mut draws = Draws(0x5EED);
    let mut fleet: Vec<Ship> = (0..3)
        .map(|local| Ship::new(local, &[9, 4, 2]).expect("config"))
        .collect();
    let mut refused = 0;
    for step in 0..4_000_u64 {
        let replica = draws.pick(3);
        let before = fleet[replica];
        let amount = draws.below(12);
        let to = draws.below(5);
        let id = step / 7 + draws.below(2);
        let other = fleet[draws.pick(3)];
        let failed = match draws.below(4) {
            0 => fleet[replica].spend(amount).is_err(),
            1 => fleet[replica].refund(amount).is_err(),
            2 => fleet[replica].transfer(to, id, amount).is_err(),
            _ => {
                let (from, total) = (draws.below(4), draws.below(12));
                // A receipt contradicting itself cannot be built at all.
                let Some(receipt) = Receipt::new(from, to, id, amount, total) else {
                    continue;
                };
                // A fabricated receipt can mint, so refuse-or-revert both ways.
                let failed = fleet[replica].accept(&receipt).is_err();
                if !failed {
                    fleet[replica] = before;
                }
                failed || fleet[replica].merge(&other).is_err()
            }
        };
        if failed {
            refused += 1;
            assert_eq!(fleet[replica], before);
        }
    }
    assert!(refused > 500);
}

#[test]
fn merge_is_commutative_associative_and_idempotent() {
    for seed in 1..=20 {
        // No final gossip here: stop the run early by using its mid-run views.
        let mut draws = Draws(seed);
        let mut fleet: Vec<Ship> = (0..3)
            .map(|local| Ship::new(local, &[20, 20, 20]).expect("config"))
            .collect();
        for step in 0..120_u64 {
            let replica = draws.pick(3);
            let to = draws.below(3);
            match draws.below(3) {
                0 => drop(fleet[replica].spend(draws.below(5))),
                1 => drop(fleet[replica].transfer(to, step, draws.below(9))),
                _ => drop(fleet[replica].refund(draws.below(3))),
            }
        }
        let [a, b, c] = [fleet[0], fleet[1], fleet[2]];
        let (mut ab, mut ba) = (a, b);
        ab.merge(&b).expect("merge");
        ba.merge(&a).expect("merge");
        assert_eq!(cells(&ab), cells(&ba));
        let (mut left, mut right, mut bc) = (ab, a, b);
        left.merge(&c).expect("merge");
        bc.merge(&c).expect("merge");
        right.merge(&bc).expect("merge");
        assert_eq!(left, right);
        let again = left;
        left.merge(&again).expect("merge");
        left.merge(&c).expect("merge");
        assert_eq!(left, again);
        let mut through_trait = a;
        Merge::merge(&mut through_trait, &b).expect("merge");
        assert_eq!(through_trait, ab);
    }
}
