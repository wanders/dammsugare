use axum::{extract::State, response::Html, response::Json, routing::get, Router};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time;

#[derive(Deserialize, Debug)]
struct Entry {
    #[serde(rename = "OutAreaElspotId")]
    area_out: String,
    #[serde(rename = "InAreaElspotId")]
    area_in: String,
    #[serde(rename = "Value")]
    value: f64,
}

#[derive(Debug)]
struct ExportState {
    curr_export: f64,
    last_update: Option<Instant>,
}

impl Entry {
    fn crosses_boundary(&self, country: &str) -> bool {
        (self.area_out.starts_with(country) && !self.area_in.starts_with(country))
            || (!self.area_out.starts_with(country) && self.area_in.starts_with(country))
    }

    fn export(&self, country: &str) -> f64 {
	/* DK2<->SE4 seems to be reversed? What am I missing? */
	if self.area_in == "DK2" && self.area_out == "SE4" {
	    return -self.value;
	}
	if self.area_in.starts_with(country) {
	    return -self.value;
	}
        self.value
    }
}

async fn fetch_one() -> Option<f64> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();

    let url = "http://driftsdata.statnett.no/restapi/PhysicalFlowMap/GetFlow";

    let res = client.get(url).send().await.ok()?;
    match res.status() {
        reqwest::StatusCode::OK => {
            println!("Status: OK, url:{:?}", res.url().path());
            let x: f64 = res
                .json::<Vec<Entry>>()
                .await
                .ok()?
                .iter()
                .filter(|e| e.crosses_boundary("SE"))
                .map(|e| e.export("SE"))
                .sum();
            println!("D: {:?}", x);

            return Some(x);
        }
        status => println!("status: {}, path: {}", status, res.url().path()),
    }
    None
}

async fn fetch(state: Arc<Mutex<ExportState>>) -> Result<(), Box<dyn std::error::Error>> {
    let mut interval = time::interval(Duration::from_secs(60));

    loop {
        interval.tick().await;
        if let Some(v) = fetch_one().await {
            let mut s = state.lock().unwrap();
            s.curr_export = v;
            s.last_update = Some(Instant::now());
        }
    }
}

async fn root() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

async fn data(State(state): State<Arc<Mutex<ExportState>>>) -> Json<Value> {
    let s = state.lock().unwrap();
    Json(json!({
    "curr_export": s.curr_export,
    "last_update": s.last_update.map(|l| l.elapsed().as_secs()),
    }))
}

async fn ebba_gron() -> ([(&'static str, &'static str); 1], &'static [u8]) {
    (
        [("Content-Type", "image/png")],
        include_bytes!("ebba-gr0n.png"),
    )
}
async fn ebba_rod() -> ([(&'static str, &'static str); 1], &'static [u8]) {
    (
        [("Content-Type", "image/png")],
        include_bytes!("ebba-r0d.png"),
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let export_state = Arc::new(Mutex::new(ExportState {
        curr_export: 0.,
        last_update: None,
    }));

    let app = Router::new()
        .route("/", get(root))
        .route("/data.json", get(data))
        .with_state(export_state.clone())
        .route("/ebba-gr0n.png", get(ebba_gron))
        .route("/ebba-r0d.png", get(ebba_rod));

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let s = axum::Server::bind(&addr).serve(app.into_make_service());

    tokio::join!(fetch(export_state), s).0?;

    Ok(())
}
