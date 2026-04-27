use anyhow::{Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Genome event from DA API SSE stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenomeEvent {
    /// Full address (e.g., "T:1745678/proto:lifi_v2/deposit:1:0xabc123")
    #[serde(default)]
    pub addr: String,
    /// Entity type (e.g., "proto", "order")
    pub entity: String,
    /// Protocol ID or order ID (e.g., "lifi_v2" or full order_id)
    #[serde(default)]
    pub id: Option<String>,
    /// Action (e.g., "deposit", "placed", "executed")
    pub action: String,
    /// Chain ID
    pub chain_id: Option<u64>,
    /// Transaction reference
    #[serde(rename = "ref_hash")]
    pub reference: Option<String>,
    /// Source chain (for cross-chain intents)
    pub src_chain: Option<u64>,
    /// Destination chain (for cross-chain intents)
    pub dst_chain: Option<u64>,
    /// Depositor address
    pub depositor: Option<String>,
    /// Recipient address (optional)
    pub recipient: Option<String>,
    /// Source token address
    pub src_token: Option<String>,
    /// Destination token address
    pub dst_token: Option<String>,
    /// Amount (as string to preserve precision)
    pub input_amount: Option<String>,
    /// Timestamp
    pub ts: Option<u64>,
    /// Protocol name (for order entities)
    pub protocol: Option<String>,
    /// Order ID (for order entities)
    pub order_id: Option<String>,
}

/// Simplified intent structure for solver
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    /// Unique intent ID
    pub id: String,
    /// Protocol name (e.g., "lifi_v2", "stargate_v2")
    pub protocol: String,
    /// Source chain ID
    pub src_chain: u64,
    /// Destination chain ID
    pub dst_chain: u64,
    /// Token address on source chain
    pub src_token: String,
    /// Token address on destination chain (might be same as src_token)
    pub dst_token: String,
    /// Amount to transfer (in token's smallest unit)
    pub amount: String,
    /// Depositor address
    pub depositor: String,
    /// Recipient address
    pub recipient: String,
    /// Transaction hash on source chain
    pub tx_hash: String,
    /// Timestamp when detected
    pub detected_at: u64,
}

impl Intent {
    /// Parse intent from genome event
    pub fn from_genome_event(event: GenomeEvent) -> Result<Self> {
        let src_chain = event
            .src_chain
            .context("Missing src_chain in genome event")?;
        let dst_chain = event
            .dst_chain
            .context("Missing dst_chain in genome event")?;

        // Support both input_amount (new) and amount (old) - with fallback to skip intent
        let protocol_name = event.protocol.as_ref().or(event.id.as_ref()).map(|s| s.as_str()).unwrap_or("unknown");

        let amount = event.input_amount
            .clone()
            .or_else(|| {
                // Genome stream might send "amount" instead of "input_amount" for older protocols
                warn!("⚠️  Protocol '{}' missing 'input_amount', this intent will be skipped", protocol_name);
                None
            })
            .context(format!("Missing input_amount field for protocol '{}' - genome stream data incomplete", protocol_name))?;

        let depositor = event
            .depositor.clone()
            .context("Missing depositor in genome event")?;

        // Recipient may be optional in some protocols
        let recipient = event.recipient.clone().unwrap_or_else(|| depositor.clone());

        // Use ref_hash as tx hash, with fallback to generated ID for protocols without tx_hash
        let tx_hash = event
            .reference.clone()
            .or_else(|| event.order_id.clone())
            .unwrap_or_else(|| {
                // Generate synthetic tx_hash for protocols that don't provide one (e.g., Li.Fi)
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};

                let mut hasher = DefaultHasher::new();
                if let Some(ref dep) = event.depositor {
                    dep.hash(&mut hasher);
                }
                event.src_chain.hash(&mut hasher);
                event.dst_chain.hash(&mut hasher);
                if let Some(ref amt) = event.input_amount {
                    amt.hash(&mut hasher);
                }
                event.ts.hash(&mut hasher);

                let synthetic_hash = format!("synthetic_{:x}", hasher.finish());
                warn!("⚠️  Generating synthetic tx_hash for protocol {:?} (missing reference): {}",
                      event.protocol.as_ref().or(event.id.as_ref()), synthetic_hash);
                synthetic_hash
            });

        // Support both src_token (new) and token (old) - with intelligent fallback
        let src_token = event.src_token
            .clone()
            .or_else(|| {
                // Fallback: infer native token (0x0) for the source chain
                warn!("⚠️  Protocol '{}' missing 'src_token', inferring native token address", protocol_name);
                Some("0x0000000000000000000000000000000000000000".to_string())
            })
            .context(format!("Missing src_token field for protocol '{}' - genome stream data incomplete", protocol_name))?;

        let dst_token = event.dst_token.unwrap_or_else(|| src_token.clone());

        // Protocol: use protocol field if available, otherwise use id
        let protocol = event.protocol.or(event.id).context("Missing protocol/id in genome event")?;

        // Generate unique intent ID from protocol + tx hash
        let id = format!("{}:{}", protocol, tx_hash);

        Ok(Intent {
            id,
            protocol,
            src_chain,
            dst_chain,
            src_token,
            dst_token,
            amount,
            depositor,
            recipient,
            tx_hash,
            detected_at: event.ts.unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64 / 1000
            }),
        })
    }
}

/// Genome SSE client
pub struct GenomeClient {
    /// SSE endpoint URL
    sse_url: String,
    /// HTTP client
    client: reqwest::Client,
}

impl GenomeClient {
    /// Create new genome client
    pub fn new(sse_url: impl Into<String>) -> Self {
        Self {
            sse_url: sse_url.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Subscribe to genome stream and send intents to channel
    pub async fn subscribe(&self, intent_tx: mpsc::Sender<Intent>) -> Result<()> {
        info!("🔌 Connecting to genome stream: {}", self.sse_url);

        loop {
            match self.subscribe_internal(&intent_tx).await {
                Ok(_) => {
                    warn!("Genome stream ended unexpectedly, reconnecting in 5s...");
                }
                Err(e) => {
                    error!("Genome stream error: {}, reconnecting in 5s...", e);
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

    async fn subscribe_internal(&self, intent_tx: &mpsc::Sender<Intent>) -> Result<()> {
        let mut response = self
            .client
            .get(&self.sse_url)
            .send()
            .await
            .context("Failed to connect to genome stream")?;

        info!("✅ Connected to genome stream");

        let mut buffer = String::new();

        while let Some(chunk) = response.chunk().await? {
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            // Process complete SSE events
            while let Some(event_end) = buffer.find("\n\n") {
                let event_text = buffer[..event_end].to_string();
                buffer = buffer[event_end + 2..].to_string();

                if let Some(intent) = self.parse_sse_event(&event_text) {
                    info!("🎯 New intent detected: {} ({} → {})", intent.protocol, intent.src_chain, intent.dst_chain);

                    if intent_tx.send(intent).await.is_err() {
                        warn!("Intent receiver dropped, stopping stream");
                        return Ok(());
                    }
                }
            }
        }

        Ok(())
    }

    fn parse_sse_event(&self, event_text: &str) -> Option<Intent> {
        // SSE format:
        // event: genome
        // data: {...}

        let mut event_type = None;
        let mut data = None;

        for line in event_text.lines() {
            if let Some(content) = line.strip_prefix("event: ") {
                event_type = Some(content.trim());
            } else if let Some(content) = line.strip_prefix("data: ") {
                data = Some(content.trim());
            }
        }

        // Accept both "genome" and "genome_entry" event types
        if event_type != Some("genome") && event_type != Some("genome_entry") {
            return None;
        }

        let data = data?;

        // Parse JSON data
        let genome_event: GenomeEvent = match serde_json::from_str(data) {
            Ok(event) => event,
            Err(e) => {
                warn!("Failed to parse genome event: {}", e);
                return None;
            }
        };

        // Accept both "proto" (old format) and "order" (new multi-protocol format)
        // Filter for deposit/placed/executed actions (cross-chain intent initiated)
        if genome_event.entity != "proto" && genome_event.entity != "order" {
            return None;
        }

        // Skip non-actionable states (only process new/pending orders)
        if genome_event.action != "deposit"
            && genome_event.action != "placed"
            && genome_event.action != "executed" {
            return None;
        }

        // Convert to Intent
        match Intent::from_genome_event(genome_event) {
            Ok(intent) => Some(intent),
            Err(e) => {
                warn!("Failed to convert genome event to intent: {}", e);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_genome_event() {
        let event_json = r#"{
            "address": "T:1745678/proto:lifi_v2/deposit:1:0xabc123",
            "entity": "proto",
            "id": "lifi_v2",
            "action": "deposit",
            "chain_id": 1,
            "ref": "0xabc123",
            "src_chain": 1,
            "dst_chain": 42161,
            "depositor": "0xuser123",
            "recipient": "0xuser123",
            "token": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
            "amount": "1000000000",
            "timestamp": 1745678400
        }"#;

        let genome_event: GenomeEvent = serde_json::from_str(event_json).unwrap();
        let intent = Intent::from_genome_event(genome_event).unwrap();

        assert_eq!(intent.protocol, "lifi_v2");
        assert_eq!(intent.src_chain, 1);
        assert_eq!(intent.dst_chain, 42161);
        assert_eq!(intent.amount, "1000000000");
    }
}
