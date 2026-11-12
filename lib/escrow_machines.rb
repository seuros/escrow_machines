# frozen_string_literal: true

require_relative 'escrow_machines/version'

module EscrowMachines
  # Proof of one transfer. `total` is everything `from` has ever granted `to`,
  # so delivering a receipt twice, late, or out of order changes nothing.
  Receipt = Data.define(:from, :to, :id, :amount, :total)
end

# Native is required: no algorithm fallback.
require 'escrow_machines_native/escrow_machines_native'
