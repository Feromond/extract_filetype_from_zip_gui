use std::error::Error;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use zip::read::ZipArchive;

use eframe::egui;
use rfd::FileDialog;

/// Specify whether the input is a file or a directory.
#[derive(PartialEq, Eq)]
enum InputType {
    File,
    Directory,
}

/// The application state for our GUI.
struct MyApp {
    input_path: String,
    /// Comma‑separated list of file extensions (e.g., "pdf, jpg, png")
    extensions: String,
    output_path: String,
    input_type: InputType,
    log: String,
}

impl Default for MyApp {
    fn default() -> Self {
        Self {
            input_path: String::new(),
            extensions: String::new(),
            output_path: String::new(),
            input_type: InputType::File,
            log: String::new(),
        }
    }
}

impl MyApp {
    /// Extract files with the specified extensions from the zip file(s)
    /// in `input_path` into `output_path`. Progress messages are appended
    /// to `self.log`.
    fn extract_files(&mut self) -> Result<(), Box<dyn Error>> {
        let input_path = PathBuf::from(&self.input_path);
        let output_path = PathBuf::from(&self.output_path);
        fs::create_dir_all(&output_path)?;

        // Split the extensions field by comma, trimming whitespace and removing any leading dot.
        let filter_exts: Vec<String> = self
            .extensions
            .split(',')
            .map(|s| s.trim().trim_start_matches('.').to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        if filter_exts.is_empty() {
            return Err("No valid file extensions provided.".into());
        }

        if self.input_type == InputType::Directory {
            if !input_path.is_dir() {
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
                    self.log
                        .push_str(&format!("Processing zip file: {}\n", path.display()));
                    process_zip_file(&path, &filter_exts, &output_path, &mut self.log)?;
                }
            }
        } else {
            if !input_path.is_file() {
                return Err(format!("{} is not a valid file.", input_path.display()).into());
            }
            self.log
                .push_str(&format!("Processing zip file: {}\n", input_path.display()));
            process_zip_file(&input_path, &filter_exts, &output_path, &mut self.log)?;
        }
        Ok(())
    }
}

/// Process a single zip file by extracting files that have an extension in `exts`.
/// Files whose names include "__MACOSX" are skipped.
/// The files are saved into `output_dir` using their original file names.
fn process_zip_file(
    zip_path: &Path,
    exts: &Vec<String>,
    output_dir: &Path,
    log: &mut String,
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

        // Only process file entries.
        if zip_file.is_file() {
            let entry_path = Path::new(entry_name);
            if let Some(entry_ext) = entry_path.extension().and_then(|s| s.to_str()) {
                let entry_ext = entry_ext.to_lowercase();
                if exts.contains(&entry_ext) {
                    if let Some(file_name) = entry_path.file_name() {
                        let output_file_path = output_dir.join(file_name);
                        let mut outfile = File::create(&output_file_path)?;
                        io::copy(&mut zip_file, &mut outfile)?;
                        log.push_str(&format!(
                            "Extracted: {}\n",
                            output_file_path.display()
                        ));
                    } else {
                        log.push_str(&format!(
                            "Warning: Skipping entry with invalid file name: {}\n",
                            entry_name
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Implement the GUI application using eframe/egui.
impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                ui.label("Extensions (comma-separated, e.g., pdf, jpg, png):");
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
            if ui.button("Extract Files").clicked() {
                match self.extract_files() {
                    Ok(_) => self.log.push_str("Extraction completed successfully.\n"),
                    Err(e) => self.log.push_str(&format!("Error: {}\n", e)),
                }
            }

            ui.separator();

            // Log output wrapped in a ScrollArea
            ui.label("Log:");
            egui::ScrollArea::vertical()
                .max_height(300.0) // This limits the height; adjust as needed
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
        Box::new(|_cc| Box::new(MyApp::default())),
    );
}
