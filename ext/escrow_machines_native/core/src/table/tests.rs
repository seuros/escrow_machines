use super::{Member, Tally};
use crate::Refusal;

#[test]
fn a_member_exists_only_below_the_capacity_and_reads_its_own_cell() {
    assert_eq!(Member::<3>::new(3), None);
    assert_eq!(Member::<3>::new(usize::MAX), None);
    assert_eq!(Member::<0>::new(0), None);
    let last = Member::<3>::new(2).expect("below capacity");
    assert_eq!((last.index(), last.id()), (2, 2));
    let mut table = [10, 20, 30];
    assert_eq!(*last.of(&table), 30);
    *last.of_mut(&mut table) = 31;
    assert_eq!(table, [10, 20, 31]);
}

#[test]
fn a_tally_raises_one_cell_refuses_overflow_untouched_and_joins_by_max() {
    let (first, last) = (
        Member::<3>::new(0).expect("member"),
        Member::<3>::new(2).expect("member"),
    );
    let mut left = Tally::<3>::new();
    assert_eq!(left.raise(last, u64::MAX - 1), Ok(u64::MAX - 1));
    assert_eq!(left.raise(last, 2), Err(Refusal::Overflow));
    assert_eq!(left.counts(), &[0, 0, u64::MAX - 1]);
    assert_eq!(left.raise(last, 1), Ok(u64::MAX));
    assert_eq!(left.raise(first, 0), Ok(0));
    assert_eq!(left.total(), i128::from(u64::MAX));

    let mut right = Tally::from_counts([4, 1, 0]);
    right.join(&left);
    assert_eq!(right, Tally::from_counts([4, 1, u64::MAX]));
    assert_eq!(right.count(first), 4);
    assert!(right.zero_from(3));
    assert!(!right.zero_from(2));
    assert!(Tally::from_counts([4, 0, 0]).zero_from(1));
}
