# frozen_string_literal: true

require 'minitest/autorun'
require 'open3'
require 'rbconfig'

# The class of the value the extension wraps (Escrow) is fixed when it loads.
# Nothing a caller does later, redefining the constant or
# `Object.const_get`, reaches an escrow being built. Each case runs in a fresh
# process: the class is fixed once per process, at load.
class ClassLookupTest < Minitest::Test
  LIB = File.expand_path('../lib', __dir__)

  PRELUDE = <<~RUBY
    require 'escrow_machines'
    E = EscrowMachines::Escrow
  RUBY

  # Runs `body` after PRELUDE in a new Ruby; answers its last line of output.
  def isolated(body)
    out, err, status = Open3.capture3(RbConfig.ruby, '-I', LIB, '-e', PRELUDE + body)
    assert status.success?, "exited #{status.exitstatus}: #{err}"
    out.lines.last&.chomp
  end

  # Bug: an Escrow constant redefined before the first escrow made it an
  # instance of the new class, which has none of the escrow's methods.
  def test_a_redefined_escrow_constant_does_not_change_the_escrow_class
    assert_equal 'true true 5 merged', isolated(<<~RUBY)
      EscrowMachines.send(:remove_const, :Escrow)
      EscrowMachines.const_set(:Escrow, Class.new)
      escrow = E.new(0, [10, 5])
      copy = E.decode(escrow.encode)
      puts [escrow.instance_of?(E), copy.instance_of?(E), escrow.spend(5), escrow.merge(E.new(1, [10, 5]))].join(' ')
    RUBY
  end

  # Bug: building the first escrow ran `Object.const_get`, Ruby code in the
  # middle of a native call. No Ruby runs while a value is built now.
  def test_building_an_escrow_runs_no_const_get
    assert_equal '0 Escrow Escrow', isolated(<<~RUBY)
      hits = 0
      Object.define_singleton_method(:const_get) do |*args|
        hits += 1
        super(*args)
      end
      escrow = E.new(0, [10, 5])
      copy = E.decode(escrow.encode)
      puts [hits, escrow.class.name.split('::').last, copy.class.name.split('::').last].join(' ')
    RUBY
  end

  # Bug: a redefined `Object.const_get` that raised hit an `unwrap` in the
  # extension: an uncatchable fatal, and the process exited.
  def test_a_raising_const_get_does_not_kill_the_process
    assert_equal '10 10', isolated(<<~RUBY)
      Object.define_singleton_method(:const_get) { |*| raise IOError, 'lookup ran in a native call' }
      escrow = E.new(0, [10, 5])
      puts [escrow.balance(0), E.decode(escrow.encode).balance(0)].join(' ')
    RUBY
  end
end
