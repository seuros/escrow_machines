# frozen_string_literal: true

require_relative 'escrow_machines/version'

module EscrowMachines
  # Proof of one transfer. `total` is everything `from` has ever granted `to`,
  # so delivering a receipt twice, late, or out of order changes nothing.
  # Like the core's `Receipt::new`, it cannot be built contradicting itself:
  # `from == to`, `amount` zero or `amount` above `total` raise RangeError,
  # a non-Integer TypeError.
  Receipt = Data.define(:from, :to, :id, :amount, :total) do
    def initialize(from:, to:, id:, amount:, total:)
      from, to, id, amount, total = EscrowMachines.__send__(:__receipt, from, to, id, amount, total)
      super
    end
  end

  # What a transfer request did: one of two values, each carrying the
  # `receipt` to deliver, which must be a Receipt (TypeError otherwise).
  module Transfer
    # The rights moved now.
    Applied = Data.define(:receipt) do
      def initialize(receipt:)
        super(receipt: EscrowMachines.__send__(:__transfer, receipt))
      end
    end
    # This id already moved these rights; nothing moved again.
    Replayed = Data.define(:receipt) do
      def initialize(receipt:)
        super(receipt: EscrowMachines.__send__(:__transfer, receipt))
      end
    end
  end

  # The latest transfer one replica made to another. Like the core's
  # `Mark::new`, an `amount` of zero raises RangeError.
  Mark = Data.define(:id, :amount) do
    def initialize(id:, amount:)
      id, amount = EscrowMachines.__send__(:__mark, id, amount)
      super
    end
  end
end

# Native is required: no algorithm fallback. It validates and builds the Data
# values above, so they must exist first.
require 'escrow_machines_native/escrow_machines_native'

module EscrowMachines
  private_class_method :__receipt, :__mark, :__transfer

  # The values are final. `initialize` is where each validates, and a subclass
  # could override it, or `allocate` build an instance that never ran it:
  # subclassing raises TypeError (as subclassing Class does) and `allocate`
  # is gone. `new`, keywords, `with` and `dup` all still run the check.
  [Receipt, Transfer::Applied, Transfer::Replayed, Mark].each do |value|
    value.singleton_class.undef_method(:allocate)
    value.define_singleton_method(:inherited) { |_| raise TypeError, "#{value} cannot be subclassed" }
  end
end
