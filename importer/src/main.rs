use anyhow::{Context, Result};
use clap::Parser;
use csv::Reader;
use influxdb::{Client, InfluxDbWriteable, Timestamp};
use log::{info, error, debug};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{File};
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use walkdir::WalkDir;

// Dynamic record structure for any CSV format
#[derive(Debug, Deserialize, Serialize, Clone)]
struct DynamicRecord {
    // Every CSV must have a timestamp column
    timestamp: String,
    // Remaining fields will be stored in this map
    #[serde(flatten)]
    fields: HashMap<String, String>,
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
        
        // Add all fields
        for (key, value) in self.fields {
            // Try to parse as number for fields
            if let Ok(float_val) = value.parse::<f64>() {
                query = query.add_field(&key, float_val);
            } else {
                // Use as tag if not a number
                query = query.add_tag(&key, value);
            }
        }
        
        query
    }
}

// Structure to store file metadata for caching
#[derive(Debug, Serialize, Deserialize, Clone)]
struct FileMetadata {
    path: String,
    hash: String,
    last_processed: chrono::DateTime<chrono::Utc>,
    records_count: usize,
}

// Structure to track insertion statistics
#[derive(Debug, Default)]
struct ImportStats {
    files_found: usize,
    files_processed: usize,
    files_skipped: usize,
    records_processed: usize,
    successful_inserts: usize,
    failed_inserts: usize,
}

/// CSV Importer for InfluxDB - processes CSV files and imports data into InfluxDB
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Directory to scan for CSV files
    #[arg(short, long, default_value = ".")]
    scan_dir: PathBuf,
    
    /// InfluxDB URL
    #[arg(short, long, default_value = "http://127.0.0.1:8086")]
    url: String,
    
    /// InfluxDB database name
    #[arg(short = 'b', long, default_value = "cursed_stats")]
    db_name: String,
    
    /// Measurement name for the data
    #[arg(short, long, default_value = "stats")]
    measurement: String,
    
    /// Number of scanner threads
    #[arg(long, default_value_t = 2)]
    scanner_threads: usize,
    
    /// Number of parser threads
    #[arg(long, default_value_t = 4)]
    parser_threads: usize,
    
    /// Number of DB writer threads
    #[arg(long, default_value_t = 4)]
    db_threads: usize,
    
    /// Channel buffer size
    #[arg(long, default_value_t = 100_000)]
    buffer_size: usize,
    
    /// Path to the cache file
    #[arg(long, default_value = ".import_cache.json")]
    cache_file: PathBuf,
    
    /// Force re-processing of all files even if in cache
    #[arg(long)]
    force: bool,
    
    /// Path to log file (empty to disable file logging)
    #[arg(long, default_value = "importer.log")]
    log_file: PathBuf,
    
    /// Enable console logging (in addition to file logging if configured)
    #[arg(long)]
    console: bool,
}

fn main() -> Result<()> {
    // Parse command line arguments
    let args = Cli::parse();
    
    // Set up logging
    setup_logging(&args)?;
    
    // Create shared statistics
    let stats = Arc::new(Mutex::new(ImportStats::default()));
    
    // Load file cache if it exists
    let cache = Arc::new(load_cache(&args.cache_file).unwrap_or_default());
    info!("Loaded cache with {} entries", cache.len());
    
    // Create three Tokio runtimes for different stages
    let scanner_runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(args.scanner_threads)
        .thread_name("scanner-pool")
        .enable_all()
        .build()
        .context("Failed to build scanner runtime")?;
    
    let parser_runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(args.parser_threads)
        .thread_name("parser-pool")
        .enable_all()
        .build()
        .context("Failed to build parser runtime")?;
    
    let db_runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(args.db_threads)
        .thread_name("db-pool")
        .enable_all()
        .build()
        .context("Failed to build db runtime")?;
    
    // Channels between stages
    let (file_tx, mut file_rx) = mpsc::channel::<PathBuf>(args.buffer_size);
    let (record_tx, mut record_rx) = mpsc::channel::<(Vec<DynamicRecord>, PathBuf, String)>(args.buffer_size);
    
    // Channels for shutdown coordination
    let (parser_complete_tx, parser_complete_rx) = oneshot::channel();
    let (db_complete_tx, db_complete_rx) = oneshot::channel();
    
    // Clone stats for each stage
    let db_stats = Arc::clone(&stats);
    let parser_stats = Arc::clone(&stats);
    let scanner_stats = Arc::clone(&stats);
    
    // Clone cache for each stage
    let db_cache = Arc::clone(&cache);
    let scanner_cache = Arc::clone(&cache);
    
    info!("Starting import from {} to database {} at {}", 
             args.scan_dir.display(), args.db_name, args.url);
    
    // Stage 3: InfluxDB inserter
    let db_cache_file = args.cache_file.clone();
    let measurement = args.measurement.clone();
    let _db_handle: JoinHandle<()> = db_runtime.spawn(async move {
        let client = Client::new(args.url, args.db_name);
        let mut updated_cache = (*db_cache).clone();
        
        info!("DB Writer ready, waiting for records...");
        while let Some((records, file_path, file_hash)) = record_rx.recv().await {
            info!("Received batch of {} records from {}", records.len(), file_path.display());
            
            let mut successful = 0;
            let mut failed = 0;
            
            for record in records {
                let query = record.into_query(&measurement);
                debug!("Query: {:#?}", &query);
                match client.query(query).await {
                    Ok(_) => successful += 1,
                    Err(e) => {
                        error!("Failed to insert record: {}", e);
                        failed += 1;
                    }
                }
            }
            
            // Update statistics
            {
                let mut stats = db_stats.lock().unwrap();
                stats.successful_inserts += successful;
                stats.failed_inserts += failed;
            }
            
            // Add to cache
            let path_str = file_path.to_string_lossy().to_string();
            updated_cache.insert(path_str.clone(), FileMetadata {
                path: path_str,
                hash: file_hash,
                last_processed: chrono::Utc::now(),
                records_count: successful + failed,
            });
            
            // Save cache after each file to prevent data loss
            if let Err(e) = save_cache(&db_cache_file, &updated_cache) {
                error!("Failed to save cache: {}", e);
            }
            
            info!("File processed: {} records, {} successful, {} failed", 
                     successful + failed, successful, failed);
        }
        
        info!("DB Writer finished");
        
        // Display final statistics
        let stats = db_stats.lock().unwrap();
        info!("\nImport Statistics:");
        info!("Files found:       {}", stats.files_found);
        info!("Files processed:   {}", stats.files_processed);
        info!("Files skipped:     {}", stats.files_skipped);
        info!("Records processed: {}", stats.records_processed);
        info!("Successful inserts: {}", stats.successful_inserts);
        info!("Failed inserts:    {}", stats.failed_inserts);
        
        // Final cache save
        if let Err(e) = save_cache(&db_cache_file, &updated_cache) {
            error!("Failed to save final cache: {}", e);
        }
        
        // Signal completion
        let _ = db_complete_tx.send(());
    });
    
    // Stage 2: CSV parser
    let _parser_handle: JoinHandle<()> = parser_runtime.spawn(async move {
        let record_tx = record_tx; // Take ownership
        
        info!("CSV Parser ready, waiting for files...");
        while let Some(path) = file_rx.recv().await {
            let path_str = path.display().to_string(); // For error reporting
            let record_tx = record_tx.clone(); 
            let parser_stats_clone = Arc::clone(&parser_stats);
            
            info!("Processing file: {}", path_str);
            {
                let mut stats = parser_stats.lock().unwrap();
                stats.files_processed += 1;
            }
            
            tokio::spawn(async move {
                // Calculate file hash for consistency checking
                let file_hash = match calculate_file_hash(&path) {
                    Ok(hash) => hash,
                    Err(e) => {
                        error!("Failed to calculate hash for {}: {}", path_str, e);
                        return;
                    }
                };
                
                match parse_csv_dynamic(path.clone()) {
                    Ok(records) => {
                        {
                            let mut stats = parser_stats_clone.lock().unwrap();
                            stats.records_processed += records.len();
                        }
                        
                        info!("Parsed {} records from {}", records.len(), path_str);
                        if let Err(e) = record_tx.send((records, path, file_hash)).await {
                            error!("Failed to send records: {}", e);
                        }
                    },
                    Err(e) => error!("Failed to parse CSV {}: {}", path_str, e),
                }
            });
        }
        info!("CSV Parser finished");
        
        // Signal completion
        let _ = parser_complete_tx.send(());
    });
    
    // Stage 1: File scanner
    scanner_runtime.block_on(async {
        info!("Starting scan for CSV files in {}", args.scan_dir.display());
        let force = args.force;
        
        for entry in WalkDir::new(&args.scan_dir).into_iter().filter_map(Result::ok) {
            let path = entry.path().to_owned();
            
            if path.extension().map_or(false, |ext| ext == "csv") {
                let path_str = path.to_string_lossy().to_string();
                info!("Found CSV: {}", path.display());
                
                {
                    let mut stats = scanner_stats.lock().unwrap();
                    stats.files_found += 1;
                }
                
                // Skip if already in cache and hash matches, unless force flag is set
                if !force {
                    if let Some(metadata) = scanner_cache.get(&path_str) {
                        match calculate_file_hash(&path) {
                            Ok(hash) if hash == metadata.hash => {
                                info!("Skipping already processed file: {}", path.display());
                                {
                                    let mut stats = scanner_stats.lock().unwrap();
                                    stats.files_skipped += 1;
                                }
                                continue;
                            }
                            _ => {} // Process file if hash doesn't match or can't calculate hash
                        }
                    }
                }
                
                if let Err(e) = file_tx.send(path).await {
                    error!("Failed to send file path: {}", e);
                    break;
                }
            }
        }
        
        info!("Scan completed");
        // Close the channel when done scanning
        drop(file_tx);
        
        // Wait for parser to finish
        if let Err(e) = parser_complete_rx.await {
            error!("Error waiting for parser to complete: {}", e);
        }
        
        // Wait for DB writer to finish
        if let Err(e) = db_complete_rx.await {
            error!("Error waiting for DB writer to complete: {}", e);
        }
        
        info!("All tasks completed");
    });
    
    Ok(())
}

// Set up logging to both file and console
fn setup_logging(args: &Cli) -> Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    std::env::set_var("RUST_LOG_STYLE", "always");
    
    let mut builder = pretty_env_logger::formatted_builder();
    builder.parse_filters("debug");
    
    // Configure console and file logging
    if args.console && !args.log_file.to_string_lossy().is_empty() {
        // Set up both console and file logging
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&args.log_file)?;

        // Log to both file and console using custom logic
        let console_logger = pretty_env_logger::formatted_builder()
            .parse_filters("info")
            .build();
            
        let file_logger = pretty_env_logger::formatted_builder()
            .parse_filters("debug")
            .target(pretty_env_logger::env_logger::Target::Pipe(Box::new(log_file)))
            .build();
            
        log::set_boxed_logger(Box::new(LogDispatcher {
            console: console_logger,
            file: file_logger,
        }))?;
        
        log::set_max_level(log::LevelFilter::Debug);
    } else if !args.log_file.to_string_lossy().is_empty() {
        // Only log to file
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&args.log_file)?;
            
        builder.target(pretty_env_logger::env_logger::Target::Pipe(Box::new(log_file)));
        builder.init();
    } else {
        // Only log to console
        pretty_env_logger::init();
    }
    
    Ok(())
}

// Custom logger that dispatches to both console and file
struct LogDispatcher {
    console: pretty_env_logger::env_logger::Logger,
    file: pretty_env_logger::env_logger::Logger,
}

impl log::Log for LogDispatcher {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.console.enabled(metadata) || self.file.enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        self.console.log(record);
        self.file.log(record);
    }

    fn flush(&self) {
        self.console.flush();
        self.file.flush();
    }
}

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
        
        // Process each field
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

// Helper function to calculate file hash
fn calculate_file_hash(path: &PathBuf) -> Result<String> {
    let mut file = File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    
    let mut hasher = Sha256::new();
    hasher.update(&buffer);
    let result = hasher.finalize();
    
    Ok(format!("{:x}", result))
}

// Load cache from file
fn load_cache(path: &PathBuf) -> Result<HashMap<String, FileMetadata>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    
    let file = File::open(path)?;
    let cache: HashMap<String, FileMetadata> = serde_json::from_reader(file)?;
    
    Ok(cache)
}

// Save cache to file
fn save_cache(path: &PathBuf, cache: &HashMap<String, FileMetadata>) -> Result<()> {
    let file = File::create(path)?;
    serde_json::to_writer_pretty(file, cache)?;
    
    Ok(())
}
