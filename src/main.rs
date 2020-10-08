use std::{env, error::Error, fmt, fs, io};

use chrono::{DateTime, Utc};

use influxdb::Client;
use influxdb::InfluxDbWriteable;

use scraper::{Html, Selector};

use serde::Deserialize;

use selectors::attr::CaseSensitivity;

const BASE_URL: &str =
    "https://www.boulderado.de/boulderadoweb/gym-clientcounter/index.php?mode=get&token=";

#[derive(Debug, Clone, InfluxDbWriteable)]
struct VisitorCount {
    time: DateTime<Utc>,
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Read configuration environment variables
    let host: String = match env::var("INFLUX_HOST") {
        Ok(host) => host,
        Err(_) => "localhost".to_string(),
    };

    let host = format!("http://{}:8086", host);

    let db: String = match env::var("INFLUX_DB") {
        Ok(db) => db,
        Err(_) => "visitors".to_string(),
    };
    // Establish connection to InfluxDB
    let client = Client::new(host, db);

    // Read boulderado tokens for different climbing gyms
    let url_file = fs::File::open("tokens.json")?;
    let reader = io::BufReader::new(url_file);
    let urls: Vec<LocationToken> = serde_json::from_reader(reader)?;

    let mut counts: Vec<VisitorCount> = Vec::new();

    for LocationToken { location, token } in &urls {
        let url = format!("{}{}", BASE_URL, token);

        // Fetch html body from the boulderado webpage
        let body = reqwest::get(&url).await?.text().await?;
        let doc = Html::parse_document(&body);
        let selector = Selector::parse("div[data-value].zoom").unwrap();

        let mut visitor_count = VisitorCount {
            time: Utc::now(),
            location: location.to_owned(),
            occupied: 0,
            free: 0,
        };

        for element in doc.select(&selector) {
            let deref = element.value();
            let val = deref.attr("data-value").unwrap().parse::<i32>().unwrap();

            // Depending on what class the tag has, we can deduce whether this 
            // is the amount of occupied or slots.
            if deref.has_class("actcounter", CaseSensitivity::CaseSensitive) {
                visitor_count.occupied = val;
            } else if deref.has_class("freecounter", CaseSensitivity::CaseSensitive) {
                visitor_count.free = val;
            }
        }

        counts.push(visitor_count.clone());
    }

    for count in counts {
        println!("{}", count);
        let write_res = client.query(&count.into_query("visitors")).await;
        assert!(write_res.is_ok(), "Error writing to database");
    }

    Ok(())
}
