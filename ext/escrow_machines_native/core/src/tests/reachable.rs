//! Every state a call sequence reaches, forged receipts and lost state
//! included, must still decode: `decode`'s soundness check rejects no honest
//! state.
use super::{Draws, fleet_of, pick, round_trips};
use crate::*;

/// An amount from the run's draws: mostly small, sometimes near [`u64::MAX`].
fn amount(draws: &mut Draws) -> u64 {
    match draws.below(6) {
        0 => u64::MAX - draws.below(4),
        1 => draws.next_u64(),
        _ => draws.below(6),
    }
}

/// Plays `steps` calls of every kind, lost state and recovery included, and
/// proves each state they reach still decodes, so `decode`'s soundness check
/// rejects no honest state, and each refusal changed nothing.
fn reachable_states_decode(seed: u64, allotted: &[u64], steps: usize) {
    let mut draws = Draws::new(seed);
    let replicas = allotted.len();
    let mut fleet = fleet_of(allotted);
    // Older states a replica could restart from, and every receipt issued.
    let mut saved = fleet.clone();
    let mut receipts: Vec<Receipt> = Vec::new();
    for _ in 0..steps {
        let replica = pick(&mut draws, replicas);
        let before = fleet[replica];
        let other = fleet[pick(&mut draws, replicas)];
        let refused = match draws.below(10) {
            0 | 1 => fleet[replica].spend(amount(&mut draws)).is_err(),
            2 => fleet[replica].refund(amount(&mut draws)).is_err(),
            3 | 4 => {
                let to = pick(&mut draws, replicas) as ReplicaId;
                let id = draws.below(12);
                let done = fleet[replica].transfer(to, id, amount(&mut draws));
                if let Ok(done) = done {
                    receipts.push(done.receipt());
                }
                done.is_err()
            }
            5 => {
                // A receipt nobody issued: `accept` cannot tell, so what it
                // admits is a reachable state too.
                let (from, to) = (pick(&mut draws, replicas), pick(&mut draws, replicas));
                let (amount, extra) = (amount(&mut draws), amount(&mut draws));
                let id = draws.below(12);
                Receipt::new(
                    from as ReplicaId,
                    to as ReplicaId,
                    id,
                    amount,
                    amount.saturating_add(extra),
                )
                .is_some_and(|receipt| fleet[replica].accept(&receipt).is_err())
            }
            6 => fleet[replica].merge(&other).is_err(),
            7 => fleet[replica].recover(&other).is_err(),
            8 if !receipts.is_empty() => {
                let receipt = receipts[pick(&mut draws, receipts.len())];
                fleet[replica].accept(&receipt).is_err()
            }
            _ => {
                // Lose everything since the last save, or save now.
                if draws.below(2) == 0 {
                    fleet[replica] = saved[replica];
                } else {
                    saved[replica] = fleet[replica];
                }
                false
            }
        };
        if refused {
            assert_eq!(fleet[replica], before);
        }
        round_trips(&fleet[replica]);
    }
}

#[test]
fn every_state_a_call_sequence_reaches_still_decodes() {
    for seed in 1..=60_u64 {
        let seed = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        reachable_states_decode(seed, &[4, 0, 3], 500);
        reachable_states_decode(seed ^ 1, &[0, 0, 1, 0], 500);
        reachable_states_decode(seed ^ 2, &[u64::MAX - 9, 0, 9], 500);
        reachable_states_decode(seed ^ 3, &[0, u64::MAX], 300);
    }
}
