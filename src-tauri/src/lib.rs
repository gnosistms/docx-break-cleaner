mod docx;
mod rules;

use docx::{repair_docx_path_with_progress, scan_docx_path, RepairResult, ScanResult};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::Emitter;

static REPAIR_JOBS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepairProgress {
    job_id: String,
    percent: u8,
    message: String,
}

#[tauri::command]
fn scan_docx(path: String) -> Result<ScanResult, String> {
    scan_docx_path(&path)
}

#[tauri::command]
async fn repair_docx(
    app: tauri::AppHandle,
    input_path: String,
    output_path: String,
    candidate_ids: Vec<String>,
    job_id: String,
) -> Result<RepairResult, String> {
    let cancelled = Arc::new(AtomicBool::new(false));
    REPAIR_JOBS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "Could not start the save operation.".to_string())?
        .insert(job_id.clone(), cancelled.clone());

    let event_job_id = job_id.clone();
    let task_result = tauri::async_runtime::spawn_blocking(move || {
        repair_docx_path_with_progress(
            &input_path,
            &output_path,
            &candidate_ids,
            &cancelled,
            |percent, message| {
                let _ = app.emit(
                    "repair-progress",
                    RepairProgress {
                        job_id: event_job_id.clone(),
                        percent,
                        message: message.to_string(),
                    },
                );
            },
        )
    })
    .await;

    if let Ok(mut jobs) = REPAIR_JOBS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        jobs.remove(&job_id);
    }
    task_result.map_err(|error| format!("The save operation stopped unexpectedly: {error}"))?
}

#[tauri::command]
fn cancel_repair(job_id: String) -> bool {
    REPAIR_JOBS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()
        .and_then(|jobs| jobs.get(&job_id).cloned())
        .map(|cancelled| {
            cancelled.store(true, Ordering::Relaxed);
            true
        })
        .unwrap_or(false)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            scan_docx,
            repair_docx,
            cancel_repair
        ])
        .run(tauri::generate_context!())
        .expect("error while running DOCX Break Cleaner");
}
