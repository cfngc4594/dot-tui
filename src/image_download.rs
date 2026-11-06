//! Room-authenticated metadata, then a SEPARATE anonymous download. No redirects,
//! cookies, proxy, account headers, or retry. Resolve once and pin public IPs.
use crate::{
    image_transfer::{BoundRoom, metadata_endpoint, parse_metadata},
    prompt_images::PastedImage,
};
use reqwest::Url;
pub trait Transport {
    fn metadata(&mut self, url: Url) -> Result<serde_json::Value, String>;
    fn anonymous(&mut self, url: Url) -> Result<Vec<u8>, String>;
}
pub fn fetch(t: &mut impl Transport, room: &BoundRoom, file: &str) -> Result<PastedImage, String> {
    let metadata_url = metadata_endpoint(room, file).map_err(|e| format!("元数据请求：{e}"))?;
    let value = t
        .metadata(metadata_url)
        .map_err(|e| format!("元数据请求：{e}"))?;
    let meta = parse_metadata(&value).map_err(|e| format!("元数据格式：{e}"))?;
    let bytes = t
        .anonymous(meta.download)
        .map_err(|e| format!("下载：{e}"))?;
    let (_, _, mime) =
        crate::attachments::dimensions_checked(&bytes).map_err(|e| format!("图片解码：{e}"))?;
    if mime != meta.mime {
        return Err("图片校验：元数据与实际格式不一致".into());
    }
    let mut c = crate::image_composer::ImageComposer::default();
    let name = meta
        .name
        .map(std::path::PathBuf::from)
        .and_then(|p| p.file_name().map(std::path::PathBuf::from));
    c.stage_bytes(bytes, name)
        .map_err(|e| format!("图片校验/解码：{e}"))?;
    let image = c.images.remove(0);
    image.prepare_preview_blocking();
    Ok(image)
}
pub fn public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v) => {
            let o = v.octets();
            !v.is_private()
                && !v.is_loopback()
                && !v.is_link_local()
                && !v.is_unspecified()
                && !v.is_multicast()
                && !v.is_broadcast()
                && o[0] != 0
                && o[0] < 224
                && !(o[0] == 100 && (64..=127).contains(&o[1]))
                && !(o[0] == 169 && o[1] == 254)
                && !v.is_documentation()
                && !(o[0] == 198 && (o[1] == 18 || o[1] == 19))
                && !(o[0] == 192 && o[1] == 0)
        }
        std::net::IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return public_ip(v4.into());
            }
            let s = v.segments();
            !v.is_loopback()
                && !v.is_unspecified()
                && !v.is_multicast()
                && !v.is_unique_local()
                && !v.is_unicast_link_local()
                && s[0] & 0xe000 == 0x2000
                && !(s[0] == 0x2001 && s[1] == 0x0db8)
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && s[1] < 0x200)
        }
    }
}
// Official CDN hosts only. This fallback resolves real public addresses; it
// never permits a fake/private destination through public_ip().
fn official_image_cdn(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "oaiusercontent.com" || host.ends_with(".oaiusercontent.com")
}
fn fake_ip(ip: std::net::IpAddr) -> bool {
    matches!(ip, std::net::IpAddr::V4(v) if v.octets()[0] == 198 && matches!(v.octets()[1], 18 | 19))
}
fn resolve_official_cdn_public(host: &str) -> Result<Vec<std::net::SocketAddr>, String> {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;
    if !official_image_cdn(host) {
        return Err("非官方图片域名不使用安全解析回退".into());
    }
    // Cloudflare's documented HTTPS JSON DNS endpoint and fixed public IPs:
    // developers.cloudflare.com/1.1.1.1/encryption/dns-over-https/make-api-requests/dns-json/
    // developers.cloudflare.com/1.1.1.1/ip-addresses/
    // Fresh anonymous client. Only the CDN hostname is queried: no image URL,
    // signed query parameters, account headers, cookies, or proxy credentials.
    let resolver_ips = [
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 443),
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 0, 0, 1)), 443),
    ];
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .resolve_to_addrs("cloudflare-dns.com", &resolver_ips)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|_| "无法建立图片安全解析器")?;
    let response = client
        .get("https://cloudflare-dns.com/dns-query")
        .query(&[("name", host), ("type", "A")])
        .header(reqwest::header::ACCEPT, "application/dns-json")
        .send()
        .map_err(|_| "官方CDN安全DNS请求失败；未访问非公网图片地址")?;
    if !response.status().is_success() {
        return Err("官方CDN安全DNS服务拒绝请求".into());
    }
    let value: serde_json::Value = response.json().map_err(|_| "安全DNS响应格式无效")?;
    if value.get("Status").and_then(serde_json::Value::as_u64) != Some(0) {
        return Err("安全DNS未能解析官方图片CDN".into());
    }
    let addresses: Vec<_> = value
        .get("Answer")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|r| r.get("type").and_then(serde_json::Value::as_u64) == Some(1))
        .filter_map(|r| r.get("data")?.as_str()?.parse::<IpAddr>().ok())
        .map(|ip| SocketAddr::new(ip, 443))
        .collect();
    if addresses.is_empty() || addresses.iter().any(|a| !public_ip(a.ip())) {
        return Err("安全DNS未返回可信公网图片地址；已拒绝连接".into());
    }
    Ok(addresses)
}
pub fn download_public(url: Url) -> Result<Vec<u8>, String> {
    use std::{io::Read, net::ToSocketAddrs, time::Duration};
    crate::image_transfer::validate_download_url(&url).map_err(str::to_owned)?;
    let host = url.host_str().ok_or("图片下载缺少主机")?;
    let mut ips = (host.trim_matches(['[', ']']), 443)
        .to_socket_addrs()
        .map_err(|_| "无法解析图片下载主机")?
        .take(32)
        .collect::<Vec<_>>();
    if !ips.is_empty() && ips.iter().all(|s| fake_ip(s.ip())) && official_image_cdn(host) {
        ips = resolve_official_cdn_public(host)?;
    }
    if ips.is_empty() || ips.iter().any(|s| !public_ip(s.ip())) {
        return Err("DNS返回非公网图片地址，已拒绝连接；请检查代理DNS配置".into());
    }
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, &ips)
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| "无法建立匿名下载器")?;
    // This fresh client has NO account credentials or cookie provider.
    let response = client
        .get(url)
        .send()
        .map_err(|_| "图片下载失败；不会自动重试")?;
    if !response.status().is_success() {
        return Err("图片下载被拒绝或重定向；未跟随".into());
    }
    if response
        .content_length()
        .is_some_and(|n| n > crate::attachments::MAX_FILE_BYTES as u64)
    {
        return Err("图片超过20 MiB".into());
    }
    let mut bytes = Vec::new();
    response
        .take(crate::attachments::MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "图片读取失败")?;
    crate::attachments::decode_bounded(&bytes).map_err(str::to_owned)?;
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_private_local_reserved_and_documentation_ips() {
        for s in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "192.168.1.1",
            "198.18.0.1",
            "192.0.2.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "2001:db8::1",
            "2002:7f00:1::",
        ] {
            assert!(!public_ip(s.parse().unwrap()), "{s}");
        }
        assert!(public_ip("8.8.8.8".parse().unwrap()));
    }
    #[test]
    fn fake_room_metadata_anonymous_download_and_original_preview() {
        struct Fake {
            calls: usize,
        }
        impl Transport for Fake {
            fn metadata(&mut self, url: Url) -> Result<serde_json::Value, String> {
                assert_eq!(url.path(), "/backend-api/messaging/rooms/room/files/file-1");
                self.calls += 1;
                Ok(
                    serde_json::json!({"mime_type":"image/png","name":"fake.png","download_url":"https://images.example.test/signed?x=1","image_metadata":{"width":32,"height":24}}),
                )
            }
            fn anonymous(&mut self, url: Url) -> Result<Vec<u8>, String> {
                assert_eq!(url.host_str(), Some("images.example.test"));
                self.calls += 1;
                Ok(crate::image_ingress::synthetic())
            }
        }
        let d = crate::backend::Dot {
            id: "d".into(),
            name: "n".into(),
            room: Some("room".into()),
            speakers: vec![],
        };
        let room = BoundRoom::from_unique(&[d], "room").unwrap();
        let mut fake = Fake { calls: 0 };
        let i = fetch(&mut fake, &room, "file-1").unwrap();
        assert_eq!(fake.calls, 2);
        assert_eq!(i.preview_dimensions(), Some((32, 24)));
    }
}
