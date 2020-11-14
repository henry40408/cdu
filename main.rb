require 'docopt'
require 'faraday'
require 'logger'
require_relative 'record_updater'

DOC = <<DOCOPT
Turbo Spoon

Usage:
  #{__FILE__} update <zone> <name>...
  #{__FILE__} daemon

Options:
  -h --help               Show this screen.
  --version               Show version.
DOCOPT

LOGGER = Logger.new(STDOUT)

def daemon
  token = ENV.fetch('CLOUDFLARE_TOKEN')
  zone = ENV.fetch('CLOUDFLARE_ZONE')
  record_names = ENV.fetch('CLOUDFLARE_RECORD_NAMES').split(',')
  delay = ENV.fetch('DELAY', '60').to_i

  LOGGER.info 'daemon mode, update records for the first time'
  loop do
    update(zone, record_names, verbose: true)
    LOGGER.info "sleep #{delay} seconds"
    sleep delay
  end
end

def update(zone, record_names, verbose: false)
  ip_address = Faraday.get('https://api.ipify.org/').body
  updater = RecordUpdater.new(token: ENV.fetch('CLOUDFLARE_TOKEN'), zone: zone)
  
  records = updater.update_many(record_names.map(&:strip), ip_address)
  records.each do |record|
    LOGGER.info "#{record[:name]}: #{ip_address}"
  end
end

def main
  begin
    opts = Docopt::docopt(DOC)
    if opts['daemon']
      daemon
    end
    if opts['update']
      update(opts['<zone>'], opts['<name>'])
    end
  rescue Docopt::Exit => e
    puts e.message
  end
end

main
