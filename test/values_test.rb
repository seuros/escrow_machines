# frozen_string_literal: true
require 'minitest/autorun'
require 'escrow_machines'

class ValuesTest < Minitest::Test
  E = EscrowMachines::Escrow
  Receipt = EscrowMachines::Receipt
  Applied = EscrowMachines::Transfer::Applied
  Replayed = EscrowMachines::Transfer::Replayed
  Mark = EscrowMachines::Mark

  def test_a_hand_built_receipt_is_held_to_the_core_constructor
    assert_equal [0, 1, 7, 4, 9], Receipt.new(0, 1, 7, 4, 9).deconstruct
    assert_raises(RangeError) { Receipt.new(1, 1, 7, 4, 9) }
    assert_raises(RangeError) { Receipt.new(0, 1, 7, 0, 9) }
    assert_raises(RangeError) { Receipt.new(0, 1, 7, 10, 9) }
    assert_raises(RangeError) { Receipt.new(-1, 1, 7, 4, 9) }
    assert_raises(RangeError) { Receipt.new(0, 1, 2**64, 4, 9) }
    assert_raises(TypeError) { Receipt.new(0, 1, 7, 4.0, 9) }
    assert_raises(TypeError) { Receipt.new(from: 0, to: '1', id: 7, amount: 4, total: 9) }
  end

  def test_a_hand_built_mark_is_held_to_the_core_constructor
    assert_equal Mark.new(3, 2), Mark.new(id: 3, amount: 2)
    assert_raises(RangeError) { Mark.new(3, 0) }
    assert_raises(RangeError) { Mark.new(-1, 1) }
    assert_raises(TypeError) { Mark.new(nil, 1) }
  end

  def test_a_transfer_carries_a_receipt
    receipt = Receipt.new(0, 1, 7, 4, 4)
    assert_equal receipt, Applied.new(receipt).receipt
    assert_equal receipt, Replayed.new(receipt: receipt).receipt
    [nil, 1, Mark.new(1, 1), [0, 1, 7, 4, 4]].each do |v|
      assert_raises(TypeError) { Applied.new(v) }
      assert_raises(TypeError) { Replayed.new(v) }
    end
  end

  # A subclass overriding `initialize` without `super`, `allocate`, or a call
  # to a hook itself would skip the core's check.
  def test_values_are_final_and_their_hooks_private
    [Receipt, Applied, Replayed, Mark].each do |value|
      assert_raises(TypeError) { Class.new(value) { def initialize(**) = nil } }
      assert_raises(NoMethodError) { value.allocate }
    end
    assert_raises(RangeError) { Receipt.new(0, 1, 7, 4, 4).with(to: 0) }
    assert_raises(NoMethodError) { EscrowMachines.__receipt(0, 1, 7, 4, 4) }
    assert_raises(NoMethodError) { EscrowMachines.__mark(1, 1) }
    assert_raises(NoMethodError) { EscrowMachines.__transfer(nil) }
  end

  # The answer to `transfer` is built from a copy kept only once it exists: a
  # build that raises moves no rights, and one that moves rights itself raises
  # RuntimeError with the outer transfer not applied.
  def test_a_transfer_whose_answer_cannot_be_built_moves_nothing
    left = E.new(0, [10, 0])
    before = left.encode
    redefining(Applied, ->(*) { raise IOError }) { assert_raises(IOError) { left.transfer(1, 1, 4) } }
    assert_equal before, left.encode
    reentrant = ->(original, *args) { left.spend(1).then { original.call(*args) } }
    redefining(Applied, reentrant) { assert_raises(RuntimeError) { left.transfer(1, 1, 4) } }
    assert_equal [9, nil], [left.available, left.last_transfer(0, 1)]
  end

  # A build that freezes the escrow: FrozenError, and no rights move.
  def test_a_transfer_whose_answer_freezes_the_escrow_moves_nothing
    left = E.new(0, [10, 0])
    before = left.encode
    freezing = ->(original, *args) { left.freeze.then { original.call(*args) } }
    redefining(Applied, freezing) { assert_raises(FrozenError) { left.transfer(1, 1, 4) } }
    assert_equal before, left.encode
  end

  # A frozen escrow raises FrozenError before anything is read.
  def test_a_frozen_escrow_raises_before_reading_arguments
    left = E.new(0, [10, 0]).freeze
    assert_raises(FrozenError) { left.accept(nil) }
    assert_raises(FrozenError) { left.transfer(1, 1, 4) }
    [-> { left.spend('1') }, -> { left.refund(-1) }, -> { left.transfer(1.5, 1, 4) },
     -> { left.merge(nil) }, -> { left.recover(:escrow) }].each do |call|
      assert_raises(FrozenError) { call.call }
    end
  end

  # Allotments are read as exactly an Array: a subclass is a TypeError.
  def test_allotted_must_be_exactly_an_array
    assert_raises(TypeError) { E.new(0, Class.new(Array).new([10, 0])) }
    assert_equal 10, E.new(0, [10, 0].tap(&:singleton_class)).supply
  end

  # Receipt fields are read from the struct itself: a redefined reader
  # answers nothing to the binding and is never run.
  def test_a_redefined_reader_is_ignored
    left, right = [0, 1].map { E.new(_1, [10, 0]) }
    receipt = left.transfer(1, 3, 4).receipt
    readers(Receipt, amount: -> { 10 }, total: -> { raise 'the binding ran a reader' }) do
      assert_equal :applied, right.accept(receipt)
    end
    assert_equal 4, right.available
  end

  # `merge` and `recover` read exactly an Escrow. A subclass has no instance
  # of its own (`new` builds an Escrow, the allocator is gone), and anything
  # else is a TypeError.
  def test_merge_and_recover_read_exactly_an_escrow
    sub = Class.new(E)
    assert_instance_of E, sub.new(1, [10, 0])
    assert_raises(TypeError) { sub.allocate }
    left = E.new(0, [10, 0])
    assert_equal :merged, left.merge(sub.new(1, [10, 0]))
    assert_raises(TypeError) { left.merge(Object.new) }
    assert_raises(TypeError) { left.recover(Receipt.new(0, 1, 3, 4, 4)) }
  end

  # Sealing stops accidents, not reflection: Ruby binds a named subclass's
  # constant before `inherited` raises, so the class leaks. Both forms raise.
  def test_values_are_sealed_and_a_named_subclass_leaks_its_constant
    [Receipt, Applied, Replayed, Mark].each { |value| assert_raises(TypeError) { Class.new(value) } }
    assert_raises(TypeError) { self.class.module_eval('class LeakedMark < Mark; end', __FILE__, __LINE__) }
    assert self.class.const_defined?(:LeakedMark, false), 'the constant is bound before inherited raises'
  end

  # A forged receipt never reaches the core: a leaked subclass whose instances
  # skip the check is a TypeError, a Marshal frame edited to contradict itself
  # a RangeError, and neither earns a refusal symbol.
  def test_a_forged_receipt_is_refused_at_the_boundary
    assert_raises(TypeError) { self.class.module_eval('class ForgedReceipt < Receipt; end', __FILE__, __LINE__) }
    ForgedReceipt.class_eval do
      def initialize(**fields) = Data.instance_method(:initialize).bind_call(self, **fields)
    end
    forged = ForgedReceipt.new(from: 1, to: 1, id: 7, amount: 4, total: 9)
    edited = Marshal.load(Marshal.dump(Receipt.new(0, 1, 7, 4, 9)).sub("i\x09".b, "i\x0F".b))
    assert_equal 10, edited.amount
    right = E.new(1, [10, 0])
    before = right.encode
    assert_raises(TypeError) { right.accept(forged) }
    assert_raises(TypeError) { Applied.new(forged) }
    assert_raises(RangeError) { right.accept(edited) }
    assert_raises(RangeError) { Replayed.new(edited) }
    assert_equal before, right.encode
  end

  # A singleton class opened on a genuine receipt does not change what it is.
  def test_an_opened_singleton_class_is_still_the_gems_receipt
    left, right = [0, 1].map { E.new(_1, [10, 0]) }
    receipt = left.transfer(1, 3, 4).receipt.tap(&:singleton_class)
    assert_equal Applied.new(Receipt.new(0, 1, 3, 4, 4)), Applied.new(receipt)
    assert_equal :applied, right.accept(receipt)
    assert_equal :replayed, right.accept(receipt.clone)
  end

  def test_what_the_escrow_returns_is_valid_and_equal_to_a_hand_built_one
    left = E.new(0, [10, 0])
    applied = left.transfer(1, 3, 4)
    assert_equal Applied.new(Receipt.new(0, 1, 3, 4, 4)), applied
    assert_equal Mark.new(3, 4), left.last_transfer(0, 1)
  end

  # Every raise happens before the core is called: a raising argument leaves
  # the escrow exactly as it was.
  def test_a_raising_argument_changes_nothing
    left = E.new(0, [10, 0])
    before = left.encode
    assert_raises(TypeError) { left.accept(Mark.new(1, 1)) }
    assert_raises(TypeError) { left.transfer(1, 1, 1.5) }
    assert_raises(RangeError) { left.spend(-1) }
    assert_equal before, left.encode
  end

  # The four refusals that carry a number in the core are bare symbols here;
  # the number is what a reader answers.
  def test_a_refusal_payload_is_read_from_the_escrow
    left, right = [0, 1].map { E.new(_1, [10, 0]) }
    assert_equal :insufficient, right.spend(1)
    assert_equal 0, right.available
    assert_equal :exceeds_spent, left.refund(1)
    assert_equal 0, left.outstanding(0)
    left.transfer(1, 5, 2)
    assert_equal :stale_id, left.transfer(1, 4, 2)
    assert_equal :id_conflict, left.transfer(1, 5, 3)
    assert_equal Mark.new(5, 2), left.last_transfer(0, 1) # latest id 5, amount 2
  end

  # The classes are resolved at init and pinned, so a compaction moves none
  # of them: every path that builds or checks a value still works after it.
  def test_every_value_path_survives_gc_compact
    skip 'GC.compact is not available' unless GC.respond_to?(:compact)
    GC.compact
    left, right = [0, 1].map { E.new(_1, [10, 0]) }
    applied = left.transfer(1, 3, 4)
    assert_equal Applied.new(Receipt.new(0, 1, 3, 4, 4)), applied
    assert_equal Replayed.new(applied.receipt), left.transfer(1, 3, 4)
    assert_equal :applied, right.accept(applied.receipt)
    assert_equal Mark.new(3, 4), left.last_transfer(0, 1)
  end

  private

  # Runs the block with each reader of `value` redefined, then restores it.
  def readers(value, **bodies)
    bodies.each_key { |name| value.alias_method(:"__original_#{name}", name) }
    bodies.each { |name, body| value.define_method(name, &body) }
    yield
  ensure
    bodies.each_key do |name|
      value.alias_method(name, :"__original_#{name}")
      value.remove_method(:"__original_#{name}")
    end
  end

  def redefining(klass, body)
    singleton = klass.singleton_class
    singleton.alias_method(:__original_new, :new)
    singleton.remove_method(:new)
    original = klass.method(:__original_new)
    klass.define_singleton_method(:new) { |*args| body.call(original, *args) }
    yield
  ensure
    singleton.remove_method(:new)
    singleton.alias_method(:new, :__original_new)
    singleton.remove_method(:__original_new)
  end
end
