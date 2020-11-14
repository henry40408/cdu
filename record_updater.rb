require 'cloudflare'

class RecordUpdater
  def initialize(token:, zone:)
    @token = token
    @zone = zone
  end

  def update(name, ip_address)
    Cloudflare.connect(token: @token) { |conn|
      zone = conn.zones.find_by_name(@zone)
      dns_record = zone.dns_records.find_by_name(name)
      
      proxied = !name.include?('*')
      dns_record.update_content(ip_address, proxied: proxied)
    }.wait
  end

  def update_many(names, ip_address)
    Cloudflare.connect(token: @token) { |conn|
      zone = conn.zones.find_by_name(@zone)
      names.map do |name|
        dns_record = zone.dns_records.find_by_name(name)
        proxied = !name.include?('*')
        dns_record.update_content(ip_address, proxied: proxied)
      end
    }.wait
  end
end
