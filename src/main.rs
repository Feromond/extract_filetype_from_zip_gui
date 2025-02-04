#![windows_subsystem = "windows"]

use std::error::Error;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use zip::read::ZipArchive;

use eframe::egui;
use rfd::FileDialog;

#[derive(PartialEq, Eq, Clone, Copy)]
enum InputType {
    File,
    Directory,
}

struct MyApp {
    input_path: String,
    /// Comma-separated list of file extensions (e.g., "pdf, jpg, png").
    /// If left empty, all files will be extracted.
    extensions: String,
    output_path: String,
    input_type: InputType,
    log: String,
    /// Receiver for log messages coming from the background extraction thread.
    log_rx: Option<mpsc::Receiver<String>>,
    /// Flag indicating if extraction is running.
    is_extracting: bool,
}

impl Default for MyApp {
    fn default() -> Self {
        Self {
            input_path: String::new(),
            extensions: String::new(),
            output_path: String::new(),
            input_type: InputType::File,
            log: String::new(),
            log_rx: None,
            is_extracting: false,
        }
    }
}

/// This function runs in a background thread. It performs the extraction work
/// and sends progress messages back through the provided channel.
fn extract_files_thread(
    input_path: String,
    output_path: String,
    extensions: String,
    input_type: InputType,
    sender: mpsc::Sender<String>,
) -> Result<(), Box<dyn Error>> {
    let output_path = PathBuf::from(&output_path);
    fs::create_dir_all(&output_path)?;

    // Split the extensions string into a vector.
    // If the field is left empty, the vector will be empty.
    let filter_exts: Vec<String> = extensions
        .split(',')
        .map(|s| s.trim().trim_start_matches('.').to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    // Log a message if no filtering is desired.
    if filter_exts.is_empty() {
        let _ = sender.send("No file extensions provided, extracting all files.\n".to_string());
    }

    let input_path = PathBuf::from(&input_path);
    if input_type == InputType::Directory {
        if !input_path.is_dir() {
            let _ = sender.send(format!("{} is not a valid directory.\n", input_path.display()));
            return Err(format!("{} is not a valid directory.", input_path.display()).into());
        }
        for entry in fs::read_dir(&input_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file()
                && path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.eq_ignore_ascii_case("zip"))
                    .unwrap_or(false)
            {
                let _ = sender.send(format!("Processing zip file: {}\n", path.display()));
                process_zip_file_thread(&path, &filter_exts, &output_path, &sender)?;
            }
        }
    } else {
        if !input_path.is_file() {
            let _ = sender.send(format!("{} is not a valid file.\n", input_path.display()));
            return Err(format!("{} is not a valid file.", input_path.display()).into());
        }
        let _ = sender.send(format!("Processing zip file: {}\n", input_path.display()));
        process_zip_file_thread(&input_path, &filter_exts, &output_path, &sender)?;
    }
    let _ = sender.send("Extraction completed successfully.\n".to_string());
    Ok(())
}

/// Processes a single zip file by extracting files.
/// If `exts` is empty, every file is extracted;
/// otherwise, only files whose extension (in lowercase) is in `exts` are extracted.
/// Files whose names include "__MACOSX" are skipped.
/// Extracted files are saved into `output_dir` using their original file names.
fn process_zip_file_thread(
    zip_path: &Path,
    exts: &Vec<String>,
    output_dir: &Path,
    sender: &mpsc::Sender<String>,
) -> Result<(), Box<dyn Error>> {
    let file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut zip_file = archive.by_index(i)?;
        let entry_name = zip_file.name();

        // Skip entries that are part of the "__MACOSX" metadata.
        if entry_name.contains("__MACOSX") {
            continue;
        }

        // Process only file entries.
        if zip_file.is_file() {
            let entry_path = Path::new(entry_name);

            // Decide whether to extract this file:
            // - If no extensions were specified, extract every file.
            // - Otherwise, extract only files with an extension in `exts`.
            let should_extract = if exts.is_empty() {
                true
            } else if let Some(entry_ext) = entry_path.extension().and_then(|s| s.to_str()) {
                exts.contains(&entry_ext.to_lowercase())
            } else {
                false
            };

            if should_extract {
                if let Some(file_name) = entry_path.file_name() {
                    let output_file_path = output_dir.join(file_name);
                    let mut outfile = File::create(&output_file_path)?;
                    io::copy(&mut zip_file, &mut outfile)?;
                    let _ = sender.send(format!("Extracted: {}\n", output_file_path.display()));
                } else {
                    let _ = sender.send(format!(
                        "Warning: Skipping entry with invalid file name: {}\n",
                        entry_name
                    ));
                }
            }
        }
    }
    Ok(())
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Drain any log messages coming from the background thread.
        if let Some(rx) = &self.log_rx {
            loop {
                match rx.try_recv() {
                    Ok(msg) => self.log.push_str(&msg),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        self.is_extracting = false;
                        self.log_rx = None;
                        break;
                    }
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Zip File Extractor");

            // Choose whether the input is a file or a directory.
            ui.horizontal(|ui| {
                ui.label("Input Type:");
                ui.radio_value(&mut self.input_type, InputType::File, "File");
                ui.radio_value(&mut self.input_type, InputType::Directory, "Directory");
            });

            // Input path (zip file or directory).
            ui.horizontal(|ui| {
                ui.label("Input Path:");
                ui.text_edit_singleline(&mut self.input_path);
                if ui.button("Browse").clicked() {
                    let selected = if self.input_type == InputType::File {
                        FileDialog::new().pick_file()
                    } else {
                        FileDialog::new().pick_folder()
                    };
                    if let Some(path) = selected {
                        self.input_path = path.display().to_string();
                    }
                }
            });

            // Extensions field.
            ui.horizontal(|ui| {
                ui.label("Extensions (comma-separated, e.g., pdf, jpg, png, if blank then all):");
                ui.text_edit_singleline(&mut self.extensions);
            });

            // Output directory.
            ui.horizontal(|ui| {
                ui.label("Output Directory:");
                ui.text_edit_singleline(&mut self.output_path);
                if ui.button("Browse").clicked() {
                    if let Some(path) = FileDialog::new().pick_folder() {
                        self.output_path = path.display().to_string();
                    }
                }
            });

            // Button to start extraction.
            if ui.button("Extract Files").clicked() && !self.is_extracting {
                // Clear the previous log and start extraction in a new thread.
                self.log.clear();
                let input_path = self.input_path.clone();
                let output_path = self.output_path.clone();
                let extensions = self.extensions.clone();
                let input_type = self.input_type;
                let (tx, rx) = mpsc::channel::<String>();
                self.log_rx = Some(rx);
                self.is_extracting = true;
                thread::spawn(move || {
                    let _ = extract_files_thread(input_path, output_path, extensions, input_type, tx);
                });
            }

            ui.separator();

            // Log output in a scrollable area.
            ui.label("Log:");
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.log)
                            .desired_rows(20)
                            .desired_width(600.0),
                    );
                });
        });
    }
}

fn main() { 
    let native_options = eframe::NativeOptions::default();
    let _ = eframe::run_native(
        "Zip File Extractor",
        native_options,
        Box::new(|_cc| Ok(Box::new(MyApp::default()))),
    );
}
