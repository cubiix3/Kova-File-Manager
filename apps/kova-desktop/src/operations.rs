use crate::{AppState, MainWindow, TransferItem, bridges::CommandDispatcher};
use kova_platform_windows::transfers::ConflictChoice;
use slint::ComponentHandle;

pub fn connect(app: &MainWindow, dispatcher: CommandDispatcher) -> slint::Timer {
    let offered_undo = std::rc::Rc::new(std::cell::Cell::new(None));
    let reviewed_undo = offered_undo.clone();
    let queue = dispatcher.transfers.clone();
    let weak = app.as_weak();
    app.global::<AppState>().on_request_undo(move || {
        if let (Some(ui), Some((id, label))) = (weak.upgrade(), queue.undo.next()) {
            reviewed_undo.set(Some(id));
            ui.global::<AppState>().set_undo_description(label.into());
            ui.global::<AppState>().set_undo_visible(true);
        }
    });
    let undo_dispatcher = dispatcher.clone();
    app.global::<AppState>().on_confirm_undo(move || {
        if let Some(id) = offered_undo.take() {
            undo_dispatcher.dispatch_undo(id);
        }
    });
    let queue = dispatcher.transfers.clone();
    app.global::<AppState>()
        .on_cancel_transfer(move |id| queue.cancel(id.max(0) as u64));
    let queue = dispatcher.transfers.clone();
    app.global::<AppState>()
        .on_clear_transfers(move || queue.clear_finished());
    let queue = dispatcher.transfers.clone();
    let weak = app.as_weak();
    app.global::<AppState>()
        .on_resolve_conflict(move |choice, all| {
            let Some(ui) = weak.upgrade() else { return };
            let state = ui.global::<AppState>();
            let choice = match choice {
                0 => ConflictChoice::Replace,
                1 => ConflictChoice::Skip,
                2 => ConflictChoice::KeepBoth,
                _ => ConflictChoice::Cancel,
            };
            queue.resolve(state.get_conflict_id().max(0) as u64, choice, all);
            state.set_conflict_visible(false);
        });
    let weak = app.as_weak();
    let mut last_rows = Vec::new();
    let model = std::rc::Rc::new(slint::VecModel::<TransferItem>::default());
    app.global::<AppState>()
        .set_transfers(slint::ModelRc::from(model.clone()));
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let state = ui.global::<AppState>();
            let snapshots = dispatcher.transfers.snapshots();
            let active = snapshots
                .iter()
                .filter(|transfer| !transfer.finished)
                .count();
            state.set_transfer_count(active as i32);
            state.set_undo_label(
                if active == 0 {
                    dispatcher
                        .transfers
                        .undo
                        .next()
                        .map(|(_, label)| label)
                        .unwrap_or_default()
                } else {
                    String::new()
                }
                .into(),
            );
            let action_required = snapshots.iter().any(|transfer| transfer.conflict.is_some());
            state.set_transfer_summary(
                if active == 0 {
                    String::new()
                } else {
                    format!(
                        "{active} {} · {}",
                        if active == 1 {
                            "operation"
                        } else {
                            "operations"
                        },
                        if action_required {
                            "Action required"
                        } else {
                            "In progress"
                        }
                    )
                }
                .into(),
            );
            let waiting = snapshots.iter().find_map(|transfer| {
                transfer
                    .conflict
                    .as_ref()
                    .map(|conflict| (transfer.id, conflict))
            });
            if let Some((id, conflict)) = waiting {
                let incoming = conflict.incoming.to_string_lossy();
                if state.get_conflict_id() != id as i32
                    || state.get_conflict_incoming() != incoming.as_ref()
                {
                    state.set_conflict_all(false);
                }
                state.set_conflict_id(id as i32);
                state.set_conflict_incoming(incoming.as_ref().into());
                state.set_conflict_incoming_name(
                    conflict
                        .incoming
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .as_ref()
                        .into(),
                );
                state.set_conflict_existing_name(
                    conflict
                        .existing
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .as_ref()
                        .into(),
                );
                state.set_conflict_existing(conflict.existing.to_string_lossy().as_ref().into());
                state.set_conflict_incoming_info(conflict.incoming_info.as_str().into());
                state.set_conflict_existing_info(conflict.existing_info.as_str().into());
                state.set_conflict_visible(true);
            } else {
                state.set_conflict_visible(false);
            }
            let rows = snapshots
                .iter()
                .rev()
                .map(|transfer| TransferItem {
                    id: transfer.id as i32,
                    title: match transfer.label.as_str() {
                        "copy" => "Copy",
                        "move" => "Move",
                        _ => "Delete",
                    }
                    .into(),
                    source: transfer.source.as_str().into(),
                    destination: transfer.destination.as_str().into(),
                    current: transfer.current.as_str().into(),
                    status: transfer.status.as_str().into(),
                    progress: transfer.progress.unwrap_or(0.0),
                    has_progress: transfer.progress.is_some(),
                    detail: format!(
                        "{} processed · {} files · {} items remaining in this stage",
                        crate::format_bytes(transfer.bytes),
                        transfer.files,
                        transfer.remaining
                    )
                    .into(),
                    error: transfer.error.as_str().into(),
                    running: !transfer.finished,
                })
                .collect::<Vec<_>>();
            if rows != last_rows {
                model.set_vec(rows.clone());
                last_rows = rows;
            }
        },
    );
    timer
}
