# cursed-stats

A statistics collection and visualization system using InfluxDB, Grafana, and a custom Rust importer.

## System Overview

This project provides:
- Automatic CSV file processing with dynamic column support
- Data storage in InfluxDB time-series database
- Visualization through Grafana dashboards
- Multi-threaded processing for high performance
- Intelligent file caching to avoid duplicate processing

## CSV Format Support

The importer supports any CSV file format, with the following requirements:
- Each CSV must have a `timestamp` column with RFC3339-formatted timestamps (e.g., `2025-04-07T20:11:15Z`)
- All other columns are automatically processed:
  - Numeric values become InfluxDB fields (for metrics)
  - String values become InfluxDB tags (for metadata)

Example CSV formats:

1. Memory metrics:
```
timestamp,usage_mb
2025-04-07T20:11:15Z,1751.3
2025-04-07T20:11:30Z,1272.5
```

2. Network metrics:
```
timestamp,download_mbps,upload_mbps,latency_ms,packet_loss
2025-04-07T20:11:15Z,5.3,2.0,42.8,3.0
2025-04-07T20:11:30Z,7.2,3.1,86.8,1.8
```

3. Disk metrics:
```
timestamp,read_mbps,write_mbps,iops,queue_depth,usage_percent
2025-04-07T20:11:15Z,91.4,12.5,205.8,3.2,67.3
```

## Setup with Docker Compose

This project uses Docker Compose to set up:
- InfluxDB 1.8 for time-series data storage
- Grafana for visualization dashboards
- A custom Rust importer service

### Prerequisites

- Docker and Docker Compose installed on your system

### Running the services

1. Configure environment variables (optional):
   ```bash
   # Configure input directory for CSV files
   export CSV_INPUT_DIR=./data
   
   # Configure importer parameters
   export MEASUREMENT=stats
   export SCANNER_THREADS=2
   export PARSER_THREADS=4
   export DB_THREADS=4
   export BUFFER_SIZE=100000
   
   # Force rebuild of the importer container
   export CACHEBUST=$(date +%s)
   ```

2. Build and start all services:
   ```bash
   docker-compose up -d
   ```

3. Place your CSV files in the configured input directory (default: `./data`)

4. Access the Grafana dashboard:
   - URL: http://localhost:3000
   - Username: admin
   - Password: admin

5. InfluxDB details:
   - URL: http://localhost:8086
   - Database name: cursed_stats
   - Username: stats_user
   - Password: stats_password

### Configuring InfluxDB as a Data Source in Grafana

1. Log in to Grafana at http://localhost:3000
2. Go to Configuration > Data Sources
3. Click "Add data source" and select "InfluxDB"
4. Use these settings:
   - Name: InfluxDB
   - URL: http://influxdb:8086 (uses Docker network name)
   - Database: cursed_stats
   - User: stats_user
   - Password: stats_password
   - HTTP Method: GET

5. Click "Save & Test" to verify the connection

### Environment Variables

The Docker Compose setup supports the following environment variables:

| Variable | Description | Default |
|----------|-------------|---------|
| CSV_INPUT_DIR | Directory on host to mount for CSV files | ./data |
| MEASUREMENT | Measurement name in InfluxDB | stats |
| SCANNER_THREADS | Number of file scanner threads | 2 |
| PARSER_THREADS | Number of CSV parser threads | 4 |
| DB_THREADS | Number of database writer threads | 4 |
| BUFFER_SIZE | Channel buffer size for processing | 100000 |
| CACHE_FILE | Path to cache file | /app/.import_cache.json |
| LOG_FILE | Path to log file | /app/importer.log |
| CACHEBUST | Force rebuild of container | 1 |

### Stopping the services

```bash
docker-compose down
```

To remove all data volumes as well:
```bash
docker-compose down -v
```

## CLI Usage (Development)

For development, you can run the importer directly with various command-line options:

```bash
cargo run -- --scan-dir /path/to/csv/files --db-name my_stats --measurement my_metrics --console
```

Available CLI options:

- `-s, --scan-dir`: Directory to scan for CSV files (default: current directory)
- `-d, --db-url`: InfluxDB URL (default: http://localhost:8086)
- `-b, --db-name`: InfluxDB database name (default: cursed_stats)
- `-m, --measurement`: Measurement name for the data (default: stats)
- `--scanner-threads`: Number of scanner threads (default: 2)
- `--parser-threads`: Number of parser threads (default: 4)
- `--db-threads`: Number of DB writer threads (default: 4)
- `--buffer-size`: Channel buffer size (default: 100,000)
- `--log-file`: Path to log file (default: importer.log, empty to disable file logging)
- `--console`: Enable console logging (in addition to file logging if configured)
- `--force`: Force re-processing of all files even if in cache
- `--cache-file`: Path to the cache file (default: .import_cache.json)

The CLI also automatically provides:
- `-h, --help`: Help information
- `-V, --version`: Print version information

You can run `cargo run -- --help` to see the full usage information.
