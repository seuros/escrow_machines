# frozen_string_literal: true
require 'minitest/autorun'
require 'escrow_machines'

class EscrowTest < Minitest::Test
  E = EscrowMachines::Escrow
  Receipt = EscrowMachines::Receipt
  MAX = 2**64 - 1

  def fleet(rights)
    rights.each_index.map { |replica| E.new(replica: replica, rights: rights) }
  end

  def test_readers_and_compiled_capacity
    escrow = E.new(replica: 1, rights: [10, 5, 0])
    assert_equal 8, EscrowMachines::CAPACITY
    assert_equal EscrowMachines::CAPACITY, escrow.capacity
    assert_equal [1, 3, 15, 5], [escrow.replica, escrow.replicas, escrow.supply, escrow.available]
    assert_equal [10, 5, 0, nil], (0..3).map { |r| escrow.allotted(r) }
    assert_equal [10, 5, 0, nil], (0..3).map { |r| escrow.balance(r) }
    assert_equal [0, nil], [escrow.spent(1), escrow.spent(3)]
    assert_equal [0, nil], [escrow.refunded(1), escrow.refunded(3)]
    assert_equal [0, nil], [escrow.outstanding(1), escrow.outstanding(3)]
    assert_equal [0, nil, nil], [escrow.granted(0, 1), escrow.granted(0, 3), escrow.granted(3, 0)]
    assert_nil escrow.last_transfer(0, 1)
    assert_nil escrow.last_transfer(0, 3)
  end

  def test_spend_and_refund_stop_at_the_local_share
    left, right = fleet([10, 5])
    assert_equal :insufficient, left.spend(11)
    assert_equal :zero_amount, left.spend(0)
    assert_equal 3, left.spend(7)
    assert_equal :exceeds_spent, left.refund(8)
    assert_equal :zero_amount, left.refund(0)
    assert_equal 5, left.refund(2)
    assert_equal [7, 2, 5], [left.spent(0), left.refunded(0), left.outstanding(0)]
    assert_equal true, right.merge(left)
    assert_equal [5, 5], [right.balance(0), right.outstanding(0)]
    assert_equal :insufficient, right.spend(6)
    assert_equal 0, right.spend(5)
  end

  def test_transfer_is_replay_safe_and_receipts_are_idempotent
    left, right, third = fleet([10, 0, 0])
    receipt = left.transfer(to: 1, id: 7, amount: 4)
    assert_equal Receipt.new(from: 0, to: 1, id: 7, amount: 4, total: 4), receipt
    assert_predicate receipt, :frozen?
    assert_equal receipt, left.transfer(to: 1, id: 7, amount: 4)
    assert_equal 6, left.available
    assert_equal :id_conflict, left.transfer(to: 1, id: 7, amount: 5)
    assert_equal :stale_id, left.transfer(to: 1, id: 6, amount: 4)
    assert_equal :insufficient, left.transfer(to: 1, id: 8, amount: 7)
    assert_equal :zero_amount, left.transfer(to: 1, id: 8, amount: 0)
    assert_equal :self_transfer, left.transfer(to: 0, id: 8, amount: 1)
    assert_equal :unknown_replica, left.transfer(to: 3, id: 8, amount: 1)
    second = left.transfer(to: 1, id: 8, amount: 2)
    assert_equal [6, [8, 2]], [second.total, left.last_transfer(0, 1)]

    assert_equal :applied, right.accept(second)
    assert_equal :replayed, right.accept(receipt)
    assert_equal :replayed, right.accept(second)
    assert_equal [6, 6], [right.available, right.granted(0, 1)]
    assert_equal :replayed, left.accept(second)
    assert_equal :applied, third.accept(receipt)
    assert_equal [0, 6], [third.available, third.balance(0)]
    # A view that saw rights leave a replica before it saw them arrive there.
    onward = right.transfer(to: 2, id: 1, amount: 5)
    late = E.new(replica: 2, rights: [10, 0, 0])
    assert_equal :applied, late.accept(onward)
    assert_equal [5, -5], [late.available, late.balance(1)]
  end

  def test_accept_refuses_receipts_that_lie
    left, right = fleet([10, 0])
    receipt = left.transfer(to: 1, id: 1, amount: 4)
    assert_equal :applied, right.accept(receipt)
    assert_equal :id_conflict, right.accept(receipt.with(amount: 3))
    assert_equal :malformed, right.accept(receipt.with(total: 9))
    assert_equal :malformed, right.accept(receipt.with(amount: 0))
    assert_equal :unknown_replica, right.accept(receipt.with(from: 2))
    assert_equal :self_transfer, right.accept(receipt.with(from: 1))
    assert_equal :forged, left.accept(receipt.with(id: 2, total: 9))
    assert_equal :overflow, E.new(replica: 1, rights: [0, MAX]).accept(receipt)
    assert_equal 4, right.available
  end

  def test_merge_converges_and_conserves_the_supply
    ships = fleet([9, 4, 2])
    ships[0].transfer(to: 2, id: 1, amount: 5)
    ships[1].spend(3)
    ships[2].merge(ships[0])
    ships[2].spend(6)
    ships[2].refund(1)
    2.times { ships.product(ships).each { |into, other| assert_equal true, into.merge(other) } }
    assert_equal [4, 1, 2], ships.map(&:available)
    assert_equal 15, ships.sum(&:available) + ships.each_with_index.sum { |ship, r| ship.outstanding(r) }
    ships.each { |ship| assert_equal [4, 1, 2], (0..2).map { |r| ship.balance(r) } }
    assert_equal :config_mismatch, ships[0].merge(E.new(replica: 0, rights: [9, 4, 3]))
    assert_equal :config_mismatch, ships[0].recover(E.new(replica: 0, rights: [9, 4]))
  end

  def test_lost_state_is_refused_until_recovered
    left, right = fleet([10, 5])
    snapshot = left.dump
    assert_equal 6, left.spend(4)
    right.merge(left)
    stale = E.load(snapshot)
    assert_equal :diverged, stale.merge(right)
    assert_equal 10, stale.available
    assert_equal true, stale.recover(right)
    assert_equal 6, stale.available

    right.accept(left.transfer(to: 1, id: 1, amount: 6))
    stale = E.load(snapshot)
    assert_equal 0, stale.spend(10)
    assert_equal :overdrawn, stale.recover(right)
    assert_equal 0, stale.available
  end

  def test_dump_and_load_round_trip
    left, = fleet([10, 5])
    left.spend(3)
    left.transfer(to: 1, id: 2, amount: 1)
    frame = left.dump
    assert_equal Encoding::BINARY, frame.encoding
    copy = E.load(frame)
    assert_equal frame, copy.dump
    assert_equal [0, 2, 6, 3, [2, 1]],
                 [copy.replica, copy.replicas, copy.available, copy.spent(0), copy.last_transfer(0, 1)]
    assert_raises(ArgumentError) { E.load(frame[0...-1]) }
    assert_raises(ArgumentError) { E.load("\x00#{frame[1..]}".b) }
    assert_raises(ArgumentError) { E.load('') }
    assert_raises(TypeError) { E.load(nil) }
    assert_raises(TypeError) { E.load(1) }
  end

  def test_u64_limits_refuse_instead_of_wrapping
    left, right = fleet([MAX, 0])
    assert_equal :applied, right.accept(left.transfer(to: 1, id: 1, amount: MAX))
    assert_equal :applied, left.accept(right.transfer(to: 0, id: MAX, amount: MAX))
    assert_equal MAX, left.available
    assert_equal :overflow, left.transfer(to: 1, id: 2, amount: 1)
    assert_equal 0, left.spend(MAX)
    assert_equal MAX, left.refund(MAX)
    assert_equal :overflow, left.spend(1)
    assert_equal :stale_id, right.transfer(to: 0, id: 0, amount: 1)
  end

  def test_validation
    [1.0, nil, '1', Float::NAN].each do |v|
      assert_raises(TypeError) { E.new(replica: v, rights: [1]) }
      assert_raises(TypeError) { E.new(replica: 0, rights: [v]) }
      assert_raises(TypeError) { E.new(replica: 0, rights: v) }
    end
    assert_raises(RangeError) { E.new(replica: -1, rights: [1]) }
    assert_raises(RangeError) { E.new(replica: 0, rights: [-1]) }
    assert_raises(RangeError) { E.new(replica: 0, rights: [2**64]) }
    assert_raises(RangeError) { E.new(replica: 0, rights: []) }
    assert_raises(RangeError) { E.new(replica: 2, rights: [1, 1]) }
    assert_raises(RangeError) { E.new(replica: 0, rights: [MAX, 1]) }
    assert_raises(RangeError) { E.new(replica: 0, rights: [1] * 9) }
    assert_equal 8, E.new(replica: 7, rights: [1] * 8).replicas
    assert_raises(ArgumentError) { E.new(replica: 0) }
    assert_raises(ArgumentError) { E.new(0, [1]) }
    assert_raises(TypeError) { E.allocate }

    escrow = E.new(replica: 0, rights: [5, 5])
    [1.5, nil, '1'].each do |v|
      assert_raises(TypeError) { escrow.spend(v) }
      assert_raises(TypeError) { escrow.refund(v) }
      assert_raises(TypeError) { escrow.transfer(to: v, id: 1, amount: 1) }
      assert_raises(TypeError) { escrow.transfer(to: 1, id: v, amount: 1) }
      assert_raises(TypeError) { escrow.transfer(to: 1, id: 1, amount: v) }
      assert_raises(TypeError) { escrow.accept(v) }
      assert_raises(TypeError) { escrow.merge(v) }
      assert_raises(TypeError) { escrow.recover(v) }
      assert_raises(TypeError) { escrow.balance(v) }
      assert_raises(TypeError) { escrow.granted(0, v) }
      assert_raises(TypeError) { escrow.accept(Receipt.new(0, 1, 1, v, 1)) }
    end
    [-1, 2**64].each do |v|
      assert_raises(RangeError) { escrow.spend(v) }
      assert_raises(RangeError) { escrow.refund(v) }
      assert_raises(RangeError) { escrow.transfer(to: 1, id: v, amount: 1) }
      assert_raises(RangeError) { escrow.allotted(v) }
      assert_raises(RangeError) { escrow.accept(Receipt.new(0, 1, v, 1, 1)) }
    end
    assert_raises(ArgumentError) { escrow.transfer(to: 1, id: 1) }
    assert_equal 5, escrow.available
  end

  def test_freeze_and_self_merge
    escrow = E.new(replica: 0, rights: [5, 5])
    receipt = escrow.transfer(to: 1, id: 1, amount: 1)
    assert_equal true, escrow.merge(escrow)
    assert_equal true, escrow.recover(escrow)
    escrow.freeze
    assert_raises(FrozenError) { escrow.spend(1) }
    assert_raises(FrozenError) { escrow.refund(1) }
    assert_raises(FrozenError) { escrow.transfer(to: 1, id: 2, amount: 1) }
    assert_raises(FrozenError) { escrow.accept(receipt) }
    assert_raises(FrozenError) { escrow.merge(escrow) }
    assert_raises(FrozenError) { escrow.recover(escrow) }
    assert_equal 4, escrow.available
    assert_equal 4, E.load(escrow.dump).available
  end

  def test_gc_stress
    previous = GC.stress
    GC.stress = true
    left, right = fleet([3, 0])
    receipt = left.transfer(to: 1, id: 1, amount: 2)
    GC.compact
    assert_equal :applied, right.accept(receipt)
    assert_equal true, left.merge(E.load(right.dump))
    assert_equal [1, 2], [left.available, right.available]
  ensure
    GC.stress = previous
  end
end
