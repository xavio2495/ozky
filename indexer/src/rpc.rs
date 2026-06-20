//! Minimal Stellar RPC client (blocking, JSON-RPC over HTTP). Only the calls the
//! indexer needs: `getLatestLedger` and `getEvents` (contract-filtered, paged).

use serde_json::{json, Value};

pub struct Rpc {
    url: String,
    agent: ureq::Agent,
}

#[derive(Clone, Debug)]
pub struct RawEvent {
    pub ledger: u32,
    pub id: String,
    pub tx_hash: String,
    /// Base64 XDR of each topic ScVal.
    pub topics: Vec<String>,
    /// Base64 XDR of the value ScVal.
    pub value: String,
}

pub struct EventsPage {
    pub events: Vec<RawEvent>,
    pub latest_ledger: u32,
    pub cursor: Option<String>,
}

impl Rpc {
    pub fn new(url: String) -> Self {
        Rpc {
            url,
            agent: ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(30))
                .build(),
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        let resp: Value = self
            .agent
            .post(&self.url)
            .send_json(body)
            .map_err(|e| format!("rpc {method} transport: {e}"))?
            .into_json()
            .map_err(|e| format!("rpc {method} decode: {e}"))?;
        if let Some(err) = resp.get("error") {
            return Err(format!("rpc {method} error: {err}"));
        }
        resp.get("result")
            .cloned()
            .ok_or_else(|| format!("rpc {method}: no result"))
    }

    pub fn latest_ledger(&self) -> Result<u32, String> {
        let r = self.call("getLatestLedger", json!({}))?;
        r.get("sequence")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .ok_or_else(|| "no sequence".into())
    }

    /// One page of contract events. Pass `cursor` to continue, else `start_ledger`.
    pub fn get_events(
        &self,
        contract_id: &str,
        start_ledger: Option<u32>,
        cursor: Option<&str>,
    ) -> Result<EventsPage, String> {
        let mut pagination = json!({ "limit": 200 });
        let mut params = json!({
            "filters": [{ "type": "contract", "contractIds": [contract_id] }],
        });
        if let Some(c) = cursor {
            pagination["cursor"] = json!(c);
        } else if let Some(s) = start_ledger {
            params["startLedger"] = json!(s);
        }
        params["pagination"] = pagination;

        let r = self.call("getEvents", params)?;
        let latest_ledger = r.get("latestLedger").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let cursor = r
            .get("cursor")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let mut events = Vec::new();
        if let Some(arr) = r.get("events").and_then(|v| v.as_array()) {
            for e in arr {
                let topics = e
                    .get("topic")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                events.push(RawEvent {
                    ledger: e.get("ledger").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    id: e.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    tx_hash: e.get("txHash").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    topics,
                    value: e.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                });
            }
        }
        Ok(EventsPage {
            events,
            latest_ledger,
            cursor,
        })
    }

    /// Resolve a usable start ledger within the RPC's retention window.
    /// Tries `latest - lookback`; if below retention, parses the floor from the
    /// error and uses that.
    pub fn resolve_start(&self, contract_id: &str, lookback: u32) -> Result<u32, String> {
        let latest = self.latest_ledger()?;
        let want = latest.saturating_sub(lookback).max(2);
        match self.get_events(contract_id, Some(want), None) {
            Ok(_) => Ok(want),
            Err(e) => {
                // "startLedger must be within the ledger range: <floor> - <latest>"
                if let Some(floor) = e
                    .split("range:")
                    .nth(1)
                    .and_then(|s| s.trim().split('-').next())
                    .and_then(|s| s.trim().parse::<u32>().ok())
                {
                    Ok(floor)
                } else {
                    Err(e)
                }
            }
        }
    }
}
