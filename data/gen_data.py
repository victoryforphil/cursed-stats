#!/usr/bin/env python3
import os
import csv
import random
import datetime
import argparse
import glob
from pathlib import Path

def generate_data(output_dir, num_points=100, interval_seconds=15):
    """
    Generate sample metrics data files with customized column structure.
    
    Args:
        output_dir: Directory to write CSV files
        num_points: Number of data points to generate per file
        interval_seconds: Time interval between data points in seconds
    """
    # Ensure output directory exists
    os.makedirs(output_dir, exist_ok=True)
    os.makedirs(os.path.join(output_dir, "subsystem"), exist_ok=True)
    
    # Clear all previous CSV files
    for csv_file in glob.glob(os.path.join(output_dir, "*.csv")):
        os.remove(csv_file)
    for csv_file in glob.glob(os.path.join(output_dir, "subsystem", "*.csv")):
        os.remove(csv_file)
    
    # Define data sources with their custom columns and value ranges
    sources = {
        "cpu_metrics.csv": {
            "columns": ["usage_percent", "temperature"],
            "ranges": [(10.0, 95.0), (35.0, 85.0)]
        },
        "memory_metrics.csv": {
            "columns": ["usage_mb"],
            "ranges": [(512.0, 2048.0)]
        },
        "network_metrics.csv": {
            "columns": ["download_mbps", "upload_mbps", "latency_ms", "packet_loss"],
            "ranges": [(0.5, 10.0), (0.2, 5.0), (5.0, 100.0), (0.0, 5.0)]
        },
        "subsystem/disk_metrics.csv": {
            "columns": ["usage_percent", "read_mbps", "write_mbps", "iops", "queue_depth"],
            "ranges": [(50.0, 95.0), (5.0, 120.0), (2.0, 80.0), (10.0, 500.0), (0.0, 10.0)]
        }
    }
    
    # Get current time and round to nearest minute
    now = datetime.datetime.now().replace(second=0, microsecond=0)
    
    # Generate data for each source
    for filename, config in sources.items():
        filepath = os.path.join(output_dir, filename)
        
        with open(filepath, 'w', newline='') as csvfile:
            writer = csv.writer(csvfile)
            
            # Create header row with timestamp and custom columns
            header = ["timestamp"] + config["columns"]
            writer.writerow(header)
            
            # Generate data points
            for i in range(num_points):
                # Calculate timestamp
                timestamp = now - datetime.timedelta(seconds=interval_seconds * (num_points - i - 1))
                timestamp_str = timestamp.strftime("%Y-%m-%dT%H:%M:%SZ")
                
                # Generate random values for each column
                row = [timestamp_str]
                for min_val, max_val in config["ranges"]:
                    value = round(random.uniform(min_val, max_val), 1)
                    row.append(value)
                
                # Write row
                writer.writerow(row)
        
        print(f"Generated {num_points} data points in {filepath} with columns: {', '.join(header)}")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Generate sample metrics data")
    parser.add_argument("-n", "--num-points", type=int, default=100,
                        help="Number of data points to generate per file (default: 100)")
    parser.add_argument("-i", "--interval", type=int, default=15,
                        help="Time interval between data points in seconds (default: 15)")
    parser.add_argument("-o", "--output-dir", type=str, default=".",
                        help="Output directory for generated files (default: current directory)")
    
    args = parser.parse_args()
    generate_data(args.output_dir, args.num_points, args.interval)


    # Usage Examples:
    #
    # 1. Generate default data (100 points at 15-second intervals in current directory):
    #    python gen_data.py
    #
    # 2. Generate 50 data points at 30-second intervals:
    #    python gen_data.py --num-points 50 --interval 30
    #
    # 3. Generate data in a specific directory:
    #    python gen_data.py --output-dir ./metrics_data
    #
    # 4. Generate 200 data points at 5-second intervals in a specific directory:
    #    python gen_data.py -n 200 -i 5 -o ./metrics_data
    #
    # This script will generate the following CSV files:
    # - cpu_metrics.csv (timestamp, usage_percent, temperature)
    # - memory_metrics.csv (timestamp, usage_mb)
    # - network_metrics.csv (timestamp, download_mbps, upload_mbps, latency_ms, packet_loss)
    # - subsystem/disk_metrics.csv (timestamp, usage_percent, read_mbps, write_mbps, iops, queue_depth)
    #
    # Each file contains a timestamp column and 1-10 custom columns with randomized values
    # within appropriate ranges for each metric type.
    # All previous CSV files in the output directory will be cleared before generating new ones.
