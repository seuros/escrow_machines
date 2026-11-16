# frozen_string_literal: true
require_relative 'lib/escrow_machines/version'

Gem::Specification.new do |spec|
  spec.name = 'escrow_machines'
  spec.version = EscrowMachines::VERSION
  spec.authors = ['Abdelkader Boudih']
  spec.email = ['oss@seuros.com']
  spec.summary = 'Bounded-counter escrow for Ruby: conserved rights, local spends, replay-safe transfers'
  spec.description = 'A fixed set of replicas shares a fixed supply; each spends only from the rights it ' \
                     'holds and moves rights by transfer, so merges in any order agree and the total spent ' \
                     'never exceeds the supply. Bound to Ruby from the no_std escrow-machines crate: the ' \
                     'accounting only, with the transport and durable storage left to the caller.'
  spec.homepage = 'https://github.com/seuros/escrow_machines'
  spec.license = 'MIT'
  spec.required_ruby_version = '>= 4.0.0'
  spec.metadata['source_code_uri'] = spec.homepage
  spec.metadata['rubygems_mfa_required'] = 'true'
  spec.metadata['cargo_crate_name'] = 'escrow_machines_native'
  spec.metadata['cargo_manifest_path'] = 'ext/escrow_machines_native/ffi/Cargo.toml'
  spec.files = Dir.glob(%w[
    lib/**/*.rb sig/**/*.rbs ext/**/*.{rs,rb,toml,md} Cargo.toml
    Cargo.lock README.md CHANGELOG.md LICENSE.txt
  ]).select { |p| File.file?(p) }
    .reject { |p| p.end_with?('/tests.rs') || p.include?('/tests/') }
  spec.require_paths = ['lib']
  spec.extensions = ['ext/escrow_machines_native/extconf.rb']
  spec.add_dependency 'rb_sys', '~> 0.9', '>= 0.9.130'
end
