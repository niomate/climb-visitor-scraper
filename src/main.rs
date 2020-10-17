extern crate chrono;
extern crate influxdb;
extern crate scraper;
extern crate serde;
extern crate thiserror;
extern crate tokio;

#[macro_use]
extern crate clap;

use chrono::{DateTime, Utc};
use influxdb::{Client, InfluxDbWriteable};
use scraper::{Html, Selector};
use serde::Deserialize;
use std::{fmt, fs, io};
use tokio::time::{self, Duration};

const BASE_URL: &str =
    "https://www.boulderado.de/boulderadoweb/gym-clientcounter/index.php?mode=get&token=";
const DB: &str = "climbing";
const DEFAULT_HOST: &str = "localhost";
const DEFAULT_IP: &str = "8086";
const DEFAULT_TOKEN_PATH: &str = "tokens.json";

#[derive(Debug, thiserror::Error)]
enum MainError {
    #[error("JsonParseError")]
    JsonParseError(#[from] serde_json::Error),
    #[error("FileError")]
    FileError(#[from] io::Error),
    #[error("SiteFetchError")]
    SiteFetchError(#[from] reqwest::Error),
    #[error("QueryError")]
    QueryError(influxdb::Error),
}

#[derive(Debug, Clone, InfluxDbWriteable)]
struct VisitorCount {
    time: DateTime<Utc>,
    #[tag]
    location: String,
    free: i32,
    occupied: i32,
}

#[derive(Deserialize, Debug)]
struct LocationToken {
    location: String,
    token: String,
}

impl fmt::Display for VisitorCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "(time: {:?}, location: {}, occupied: {}, free: {})",
            self.time, self.location, self.occupied, self.free
        )
    }
}

impl VisitorCount {
    fn from_html(location: &str, doc: &Html) -> VisitorCount {
        let occ_selector = Selector::parse("div.actcounter-content > span").unwrap();
        let free_selector = Selector::parse("div.freecounter-content > span").unwrap();

        let occupied = extract_count(&doc, &occ_selector);
        let free = extract_count(&doc, &free_selector);

        VisitorCount {
            time: Utc::now(),
            location: location.to_string(),
            free,
            occupied,
        }
    }
}

fn extract_count(doc: &Html, selector: &Selector) -> i32 {
    match doc.select(selector).next() {
        Some(elem) => elem.inner_html().parse::<i32>().unwrap(),
        None => 0,
    }
}

fn read_tokens(path: &str) -> Result<Vec<LocationToken>, MainError> {
    let url_file = fs::File::open(path)?;
    let reader = io::BufReader::new(url_file);
    let tokens: Vec<LocationToken> = serde_json::from_reader(reader)?;
    Ok(tokens)
}

async fn fetch_site_from_token(token: &str) -> Result<Html, MainError> {
    let url = format!("{}{}", BASE_URL, token);

    // Fetch html body from the boulderado webpage
    let body = reqwest::get(&url).await?.text().await?;
    Ok(Html::parse_document(&body))
}

#[tokio::main]
async fn main() -> Result<(), MainError> {
    let matches = clap_app!(climb_visitor_scraper =>
        (version: "1.0")
        (author: "Daniel Gusenburger")
        (about: "Scrape boulderado to extract visitor numbers for climbing gyms")
        (@arg HOST: -H --host +takes_value "IP address of the database host")
        (@arg IP: -p --port +takes_value "Port the database runs on")
        (@arg TOKEN_PATH: -t --token_path +takes_value "Path for the token config file")
        (@arg INTERVAL: -i --interval +takes_value "Interval in which the action is performed. 0 for running only once")
        (@arg ONCE: -o --once "Only scrape once")
    )
    .get_matches();

    let host = matches.value_of("HOST").unwrap_or(DEFAULT_HOST);
    let ip = matches.value_of("IP").unwrap_or(DEFAULT_IP);
    let token_path = matches.value_of("TOKEN_PATH").unwrap_or(DEFAULT_TOKEN_PATH);
    let interval_time = matches.value_of("INTERVAL").map_or(60, |val_str| {
        val_str.parse::<u64>().expect("Interval must be a number.")
    });
    let once = matches.is_present("ONCE");

    let host = format!("http://{}:{}", host, ip);

    // Establish connection to MongoDB
    let client = Client::new(host, DB);

    // Read boulderado tokens for different climbing gyms
    let tokens = read_tokens(&token_path)?;

    let mut interval = time::interval(Duration::from_secs(interval_time));

    loop {
        interval.tick().await;

        for LocationToken { location, token } in &tokens {
            let html = fetch_site_from_token(token).await?;

            let visitor_count = VisitorCount::from_html(location, &html);

            println!("{}", visitor_count);
            client
                .query(&visitor_count.into_query("visitors"))
                .await
                .map_err(MainError::QueryError)?;
        }

        if once {
            break Ok(());
        }
    }
}
