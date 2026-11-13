# frozen_string_literal: true

require_relative 'escrow_machines/version'

module EscrowMachines
  # Proof of one transfer. `total` is everything `from` has ever granted `to`,
  # so delivering a receipt twice, late, or out of order changes nothing.
  Receipt = Data.define(:from, :to, :id, :amount, :total)

  # What a transfer request did: one of two values, each carrying the
  # `receipt` to deliver.
  module Transfer
    # The rights moved now.
    Applied = Data.define(:receipt)
    # This id already moved these rights; nothing moved again.
    Replayed = Data.define(:receipt)
  end

  # The latest transfer one replica made to another.
  Mark = Data.define(:id, :amount)
end

# Native is required: no algorithm fallback. It builds the Data values above,
# so they must exist first.
require 'escrow_machines_native/escrow_machines_native'
