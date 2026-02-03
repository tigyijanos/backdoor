use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::models::{FileChunk, FileTransferData};

const CHUNK_SIZE: usize = 65536; // 64 KB chunks

/// File transfer manager for sending and receiving files
pub struct FileTransferManager {
    /// Active outgoing transfers (transfer_id -> transfer state)
    outgoing_transfers: HashMap<String, OutgoingTransfer>,
    /// Active incoming transfers (transfer_id -> transfer state)
    incoming_transfers: HashMap<String, IncomingTransfer>,
    /// Default download directory
    download_dir: PathBuf,
}

/// State of an outgoing file transfer
struct OutgoingTransfer {
    file_path: PathBuf,
    metadata: FileTransferData,
    chunks_sent: usize,
    acknowledged_chunks: HashSet<i32>,
    started_at: Instant,
    bytes_transferred: u64,
}

/// State of an incoming file transfer
struct IncomingTransfer {
    metadata: FileTransferData,
    received_chunks: HashMap<i32, Vec<u8>>,
    file_path: PathBuf,
    started_at: Instant,
    bytes_transferred: u64,
}

impl FileTransferManager {
    /// Create a new file transfer manager with the specified download directory
    pub fn new(download_dir: PathBuf) -> Result<Self> {
        // Ensure download directory exists
        std::fs::create_dir_all(&download_dir)?;

        Ok(Self {
            outgoing_transfers: HashMap::new(),
            incoming_transfers: HashMap::new(),
            download_dir,
        })
    }

    /// Start sending a file
    pub fn start_send(&mut self, file_path: PathBuf) -> Result<FileTransferData> {
        let metadata = File::open(&file_path)?
            .metadata()?;

        let file_size = metadata.len() as i64;
        let total_chunks = ((file_size as usize + CHUNK_SIZE - 1) / CHUNK_SIZE) as i32;

        let transfer_id = uuid::Uuid::new_v4().to_string();
        let filename = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid filename"))?
            .to_string();

        let transfer_data = FileTransferData {
            transfer_id: transfer_id.clone(),
            filename,
            file_size,
            total_chunks,
        };

        self.outgoing_transfers.insert(
            transfer_id,
            OutgoingTransfer {
                file_path,
                metadata: transfer_data.clone(),
                chunks_sent: 0,
                acknowledged_chunks: HashSet::new(),
                started_at: Instant::now(),
                bytes_transferred: 0,
            },
        );

        Ok(transfer_data)
    }

    /// Get the next chunk to send for a transfer
    pub fn get_next_chunk(&mut self, transfer_id: &str) -> Result<Option<FileChunk>> {
        let transfer = self
            .outgoing_transfers
            .get_mut(transfer_id)
            .ok_or_else(|| anyhow::anyhow!("Transfer not found"))?;

        // Find the next unacknowledged chunk
        let mut chunk_index = None;
        for i in 0..transfer.metadata.total_chunks {
            if !transfer.acknowledged_chunks.contains(&i) {
                chunk_index = Some(i);
                break;
            }
        }

        let chunk_index = match chunk_index {
            Some(idx) => idx,
            None => return Ok(None), // All chunks acknowledged
        };

        let mut file = File::open(&transfer.file_path)?;
        let offset = (chunk_index as usize) * CHUNK_SIZE;
        let mut buffer = vec![0u8; CHUNK_SIZE];

        file.seek(std::io::SeekFrom::Start(offset as u64))?;
        let bytes_read = file.read(&mut buffer)?;
        buffer.truncate(bytes_read);

        let checksum = calculate_checksum(&buffer);

        transfer.chunks_sent = transfer.chunks_sent.max(chunk_index as usize + 1);

        Ok(Some(FileChunk {
            transfer_id: transfer_id.to_string(),
            chunk_index,
            data: buffer,
            checksum,
        }))
    }

    /// Start receiving a file
    pub fn start_receive(&mut self, metadata: FileTransferData) -> Result<()> {
        // Sanitize filename - extract only the filename component, no paths allowed
        let sanitized_filename = Path::new(&metadata.filename)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid or malicious filename: path traversal detected"))?;

        // Additional validation: check for excessively long filenames
        if sanitized_filename.len() > 255 {
            return Err(anyhow::anyhow!("Filename too long (max 255 characters)"));
        }

        // Check for null bytes or other problematic characters
        if sanitized_filename.contains('\0') {
            return Err(anyhow::anyhow!("Invalid filename: contains null byte"));
        }

        let file_path = self.download_dir.join(sanitized_filename);

        self.incoming_transfers.insert(
            metadata.transfer_id.clone(),
            IncomingTransfer {
                metadata,
                received_chunks: HashMap::new(),
                file_path,
                started_at: Instant::now(),
                bytes_transferred: 0,
            },
        );

        Ok(())
    }

    /// Process a received chunk
    pub fn receive_chunk(&mut self, chunk: FileChunk) -> Result<bool> {
        let transfer = self
            .incoming_transfers
            .get_mut(&chunk.transfer_id)
            .ok_or_else(|| anyhow::anyhow!("Transfer not found"))?;

        // Verify checksum
        let calculated_checksum = calculate_checksum(&chunk.data);
        if calculated_checksum != chunk.checksum {
            return Err(anyhow::anyhow!("Checksum mismatch for chunk {}", chunk.chunk_index));
        }

        // Track bytes transferred
        transfer.bytes_transferred += chunk.data.len() as u64;

        transfer.received_chunks.insert(chunk.chunk_index, chunk.data);

        // Check if transfer is complete
        let is_complete = transfer.received_chunks.len() as i32 == transfer.metadata.total_chunks;

        if is_complete {
            self.finalize_receive(&chunk.transfer_id)?;
        }

        Ok(is_complete)
    }

    /// Finalize a completed file reception
    fn finalize_receive(&mut self, transfer_id: &str) -> Result<()> {
        let transfer = self
            .incoming_transfers
            .remove(transfer_id)
            .ok_or_else(|| anyhow::anyhow!("Transfer not found"))?;

        let mut file = File::create(&transfer.file_path)?;

        // Write chunks in order
        for i in 0..transfer.metadata.total_chunks {
            let chunk_data = transfer
                .received_chunks
                .get(&i)
                .ok_or_else(|| anyhow::anyhow!("Missing chunk {}", i))?;
            file.write_all(chunk_data)?;
        }

        file.flush()?;

        Ok(())
    }

    /// Acknowledge a chunk that was successfully received by the peer
    pub fn acknowledge_chunk(&mut self, transfer_id: &str, chunk_index: i32) -> Result<()> {
        let transfer = self
            .outgoing_transfers
            .get_mut(transfer_id)
            .ok_or_else(|| anyhow::anyhow!("Transfer not found"))?;

        transfer.acknowledged_chunks.insert(chunk_index);

        // Track bytes transferred
        transfer.bytes_transferred += CHUNK_SIZE as u64;

        Ok(())
    }

    /// Get the progress of an outgoing transfer (0.0 to 1.0)
    pub fn get_send_progress(&self, transfer_id: &str) -> Option<f32> {
        self.outgoing_transfers.get(transfer_id).map(|transfer| {
            transfer.acknowledged_chunks.len() as f32 / transfer.metadata.total_chunks as f32
        })
    }

    /// Get the progress of an incoming transfer (0.0 to 1.0)
    pub fn get_receive_progress(&self, transfer_id: &str) -> Option<f32> {
        self.incoming_transfers.get(transfer_id).map(|transfer| {
            transfer.received_chunks.len() as f32 / transfer.metadata.total_chunks as f32
        })
    }

    /// Cancel an outgoing transfer
    pub fn cancel_send(&mut self, transfer_id: &str) -> bool {
        self.outgoing_transfers.remove(transfer_id).is_some()
    }

    /// Cancel an incoming transfer
    pub fn cancel_receive(&mut self, transfer_id: &str) -> bool {
        self.incoming_transfers.remove(transfer_id).is_some()
    }

    /// Get the download directory path
    pub fn download_dir(&self) -> &Path {
        &self.download_dir
    }

    /// Get list of active outgoing transfers with their metadata and progress
    pub fn get_outgoing_transfers(&self) -> Vec<(String, FileTransferData, f32)> {
        self.outgoing_transfers
            .iter()
            .map(|(id, transfer)| {
                let progress = transfer.acknowledged_chunks.len() as f32 / transfer.metadata.total_chunks as f32;
                (id.clone(), transfer.metadata.clone(), progress)
            })
            .collect()
    }

    /// Get list of active incoming transfers with their metadata and progress
    pub fn get_incoming_transfers(&self) -> Vec<(String, FileTransferData, f32)> {
        self.incoming_transfers
            .iter()
            .map(|(id, transfer)| {
                let progress = transfer.received_chunks.len() as f32 / transfer.metadata.total_chunks as f32;
                (id.clone(), transfer.metadata.clone(), progress)
            })
            .collect()
    }

    /// Get transfer speed in bytes per second for an outgoing transfer
    pub fn get_send_speed(&self, transfer_id: &str) -> Option<f64> {
        self.outgoing_transfers.get(transfer_id).map(|transfer| {
            let elapsed = transfer.started_at.elapsed().as_secs_f64();
            if elapsed > 0.0 {
                transfer.bytes_transferred as f64 / elapsed
            } else {
                0.0
            }
        })
    }

    /// Get transfer speed in bytes per second for an incoming transfer
    pub fn get_receive_speed(&self, transfer_id: &str) -> Option<f64> {
        self.incoming_transfers.get(transfer_id).map(|transfer| {
            let elapsed = transfer.started_at.elapsed().as_secs_f64();
            if elapsed > 0.0 {
                transfer.bytes_transferred as f64 / elapsed
            } else {
                0.0
            }
        })
    }
}

/// Calculate SHA-256 checksum of data
fn calculate_checksum(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}
