use bson::doc;
use chrono::{DateTime, Utc};
use mongodb::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::{env, error::Error, fmt, fs, io};

const BASE_URL: &str =
    "https://www.boulderado.de/boulderadoweb/gym-clientcounter/index.php?mode=get&token=";
const DB: &str = "climbing";
const COLLECTION: &str = "visitors";

#[derive(Debug, Clone, Serialize)]
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Read configuration environment variables
    let host: String = match env::var("MONGO_HOST") {
        Ok(host) => host,
        Err(_) => "localhost".to_string(),
    };

    let host = format!("mongodb://{}:27017", host);

    let tokens = match env::var("CLIMBING_TOKENS") {
        Ok(tokens) => tokens,
        Err(_) => "tokens.json".to_string(),
    };

    // Establish connection to MongoDB
    let client = Client::with_uri_str(&host).await?;
    let db = client.database(DB);
    let collection = db.collection(COLLECTION);

    // Read boulderado tokens for different climbing gyms
    let url_file = fs::File::open(tokens)?;
    let reader = io::BufReader::new(url_file);
    let urls: Vec<LocationToken> = serde_json::from_reader(reader)?;

    for LocationToken { location, token } in &urls {
        let url = format!("{}{}", BASE_URL, token);

        // Fetch html body from the boulderado webpage
        let body = reqwest::get(&url).await?.text().await?;
        let doc = Html::parse_document(&body);

        let visitor_count = VisitorCount::from_html(location, &doc);

        println!("{}", visitor_count);
        let bson_doc = bson::to_document(&visitor_count)?;
        collection.insert_one(bson_doc, None).await?;
    }

    Ok(())
}
