//! Price feed (public market data; independent of the shielded pool). Fetches USD spot
//! prices + history from CoinGecko's free API for the wallet's assets, so the UI can show
//! USD values and price charts. Network I/O — call the commands off the UI thread.

use super::CoreError;
use serde::Serialize;
use serde_json::Value;

const CG: &str = "https://api.coingecko.com/api/v3";

/// CoinGecko coin id for an ozky asset code. USDC/EURC track fiat but still have a
/// real (≈1) market price; XLM floats.
fn coin_id(code: &str) -> Option<&'static str> {
    match code {
        "XLM" => Some("stellar"),
        "USDC" => Some("usd-coin"),
        "EURC" => Some("euro-coin"),
        _ => None,
    }
}

#[derive(Serialize, Clone)]
pub struct Spot {
    pub code: String,
    pub usd: f64,
    /// 24h change in percent (may be 0 if unavailable).
    pub change_24h: f64,
}

#[derive(Serialize, Clone)]
pub struct Point {
    /// Unix milliseconds.
    pub t: i64,
    pub usd: f64,
}

fn get_json(url: &str) -> Result<Value, CoreError> {
    ureq::get(url)
        .call()
        .map_err(|e| CoreError::Chain(format!("price fetch: {e}")))?
        .into_json()
        .map_err(|e| CoreError::Chain(format!("price decode: {e}")))
}

/// Current USD spot + 24h change for the given asset codes.
pub fn spot(codes: &[String]) -> Result<Vec<Spot>, CoreError> {
    let ids: Vec<&str> = codes.iter().filter_map(|c| coin_id(c)).collect();
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let url = format!(
        "{CG}/simple/price?ids={}&vs_currencies=usd&include_24hr_change=true",
        ids.join(",")
    );
    let v = get_json(&url)?;
    let mut out = Vec::new();
    for code in codes {
        if let Some(id) = coin_id(code) {
            let node = v.get(id);
            let usd = node.and_then(|n| n.get("usd")).and_then(|x| x.as_f64()).unwrap_or(0.0);
            let change = node
                .and_then(|n| n.get("usd_24h_change"))
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            out.push(Spot { code: code.clone(), usd, change_24h: change });
        }
    }
    Ok(out)
}

/// USD price history for one asset over `days` (CoinGecko market_chart).
pub fn history(code: &str, days: u32) -> Result<Vec<Point>, CoreError> {
    let id = coin_id(code).ok_or_else(|| CoreError::Chain(format!("unknown asset {code}")))?;
    let days = days.clamp(1, 365);
    let url = format!("{CG}/coins/{id}/market_chart?vs_currency=usd&days={days}");
    let v = get_json(&url)?;
    let prices = v
        .get("prices")
        .and_then(|p| p.as_array())
        .ok_or_else(|| CoreError::Chain("price history: no prices".into()))?;
    let mut out = Vec::with_capacity(prices.len());
    for pair in prices {
        if let Some(a) = pair.as_array() {
            let t = a.first().and_then(|x| x.as_f64()).unwrap_or(0.0) as i64;
            let usd = a.get(1).and_then(|x| x.as_f64()).unwrap_or(0.0);
            out.push(Point { t, usd });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coin_ids_map_known_assets() {
        assert_eq!(coin_id("XLM"), Some("stellar"));
        assert_eq!(coin_id("USDC"), Some("usd-coin"));
        assert_eq!(coin_id("???"), None);
    }

    #[test]
    #[ignore = "live network (CoinGecko)"]
    fn spot_and_history_fetch() {
        let s = spot(&["XLM".into(), "USDC".into()]).unwrap();
        assert!(s.iter().any(|x| x.code == "XLM" && x.usd > 0.0));
        let h = history("XLM", 7).unwrap();
        assert!(h.len() > 2);
    }
}
