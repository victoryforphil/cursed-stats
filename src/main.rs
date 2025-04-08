// Stage 3: InfluxDB inserter
let db_cache_file = args.cache_file.clone();
let measurement = args.measurement.clone();
let _db_handle: JoinHandle<()> = db_runtime.spawn(async move {
    // ...existing code...
});

// Stage 2: CSV parser
let _parser_handle: JoinHandle<()> = parser_runtime.spawn(async move {
    // ...existing code...
});

// Helper function to parse CSV files with dynamic columns
fn parse_csv_dynamic(path: PathBuf) -> Result<Vec<DynamicRecord>> {
    let mut records = Vec::new();
    let mut reader = Reader::from_path(&path)?;
    
    // Get headers first
    let headers = reader.headers()?.clone();
    
    // Process each record manually
    for result in reader.records() {
        let csv_record = result?;
        let mut record = DynamicRecord {
            timestamp: String::new(),
            fields: HashMap::new(),
        };
        
        // Process each field using column name as field/tag name
        for (i, field) in csv_record.iter().enumerate() {
            if i < headers.len() {
                let header = &headers[i];
                if header == "timestamp" {
                    record.timestamp = field.to_string();
                } else {
                    record.fields.insert(header.to_string(), field.to_string());
                }
            }
        }
        
        if !record.timestamp.is_empty() {
            records.push(record);
        } else {
            error!("Skipping record without timestamp");
        }
    }
    
    Ok(records)
}

impl InfluxDbWriteable for DynamicRecord {
    fn into_query<S: Into<String>>(self, measurement: S) -> influxdb::WriteQuery {
        // Create a timestamp for InfluxDB
        let ts = if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&self.timestamp) {
            let utc_dt = dt.with_timezone(&chrono::Utc);
            // Convert to u128 for nanoseconds
            if let Some(nanos) = utc_dt.timestamp_nanos_opt() {
                Timestamp::Nanoseconds(nanos as u128)
            } else {
                // Fallback to seconds
                Timestamp::Seconds(utc_dt.timestamp() as u128)
            }
        } else {
            // Use current time if we can't parse the timestamp
            let now = chrono::Utc::now();
            Timestamp::Nanoseconds(now.timestamp_nanos_opt().unwrap_or(0) as u128)
        };
        
        // Create the write query with measurement and timestamp
        let mut query = influxdb::WriteQuery::new(ts, measurement);
        
        // Process each field/tag based on its value
        for (key, value) in self.fields {
            // Try to parse as number for fields
            if let Ok(float_val) = value.parse::<f64>() {
                // If it's a number, use it as a field
                query = query.add_field(&key, float_val);
            } else {
                // If it's not a number, use it as a tag
                query = query.add_tag(&key, value);
            }
        }
        
        query
    }
}

// Dynamic record structure for any CSV format
#[derive(Debug, Deserialize, Serialize, Clone)]
struct DynamicRecord {
    // Every CSV must have a timestamp column
    timestamp: String,
    // Remaining fields will be stored in this map
    #[serde(flatten)]
    fields: HashMap<String, String>,
} 