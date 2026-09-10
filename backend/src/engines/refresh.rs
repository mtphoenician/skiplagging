use crate::config::Settings;
use crate::fx::offer_in_usd;
use crate::models::Offer;
use crate::providers::duffel::DuffelProvider;
use crate::providers::mock::MockProvider;

fn sandbox(offer: &Offer) -> bool {
    crate::providers::sandbox::is_sandbox_offer(offer)
}

pub async fn refresh_priced_offer(
    offer: &Offer,
    settings: &Settings,
    client: Option<&reqwest::Client>,
) -> Option<Offer> {
    let source = offer.source.to_lowercase();
    let oid = offer.id.as_str();
    let fresh = if source == "mock" || oid.starts_with("mock-") {
        MockProvider::new().refresh_offer(offer).await.ok().flatten()
    } else if source == "duffel" || oid.starts_with("duffel-") {
        if !settings.duffel_enabled() {
            return None;
        }
        let client = client?;
        let fresh = DuffelProvider::new(settings, client.clone())
            .refresh_offer(offer)
            .await
            .ok()
            .flatten();
        fresh.filter(|o| !sandbox(o))
    } else {
        return None;
    };
    offer_in_usd(&fresh?)
}
