use chrono::{DateTime, Utc};
use influxdb::InfluxDbWriteable;
use influxdb::{Client, Query};
use scraper::{Html, Selector};
use serde::Deserialize;
use std::{error::Error, fmt, fs, io};

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
    let url_file = fs::File::open("tokens.json")?;
    let reader = io::BufReader::new(url_file);
    let urls: Vec<LocationToken> = serde_json::from_reader(reader)?;

    // let mut urls = HashMap::new();
    let client = Client::new("http://172.17.0.2:8086", "visitors");

    let mut counts: Vec<VisitorCount> = Vec::new();

    for LocationToken { location, token } in &urls {
        let url = format!("{}{}", BASE_URL, token);

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

            for class in deref.classes() {
                match class {
                    "actcounter" => visitor_count.occupied = val,
                    "freecounter" => visitor_count.free = val,
                    _ => (),
                }
            }
        }

        counts.push(visitor_count.clone());
    }

    for count in counts {
        println!("{}", count);
        let write_res = client.query(&count.into_query("visitors")).await;
        assert!(write_res.is_ok(), "Error writing to database");
    }

    // Let's see if the data we wrote is there
    let read_query = Query::raw_read_query("SELECT * FROM visitors");

    let read_result = client.query(&read_query).await;
    assert!(read_result.is_ok(), "Read result was not ok");
    println!("{}", read_result.unwrap());

    Ok(())
}
