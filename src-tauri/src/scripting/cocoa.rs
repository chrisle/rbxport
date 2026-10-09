//! Cocoa Scripting: the classes `rbxport.sdef` names, and the accessors it
//! gives the application object.
//!
//! Cocoa reads and writes a scripting object through key-value coding, so
//! each class here overrides `valueForKey:` and `setValue:forKey:` and answers
//! the dictionary's keys itself, passing anything else to `NSObject`. The
//! application object is AppKit's, not ours, so its keys are added to
//! `NSApplication` as methods of their own at start.
//!
//! Every call arrives on the main thread. Reads are answered there, from the
//! backend's in-memory state. Anything that writes, or waits on the window,
//! suspends the command, runs off the main thread, and resumes the command
//! with the result when it is done: a script waits, the window does not.

#![allow(
    unsafe_code,
    reason = "Cocoa Scripting is Objective-C: its classes are defined, and NSApplication extended, through the objc2 runtime"
)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::CStr;
use std::future::Future;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Imp, NSObject, Sel};
use objc2::{
    define_class, msg_send, sel, AllocAnyThread, ClassType, DefinedClass, DowncastTarget, MainThreadMarker, MainThreadOnly,
    Message,
};
use objc2_app_kit::NSApplication;
use objc2_foundation::{
    NSAppleEventDescriptor, NSArray, NSIndexSpecifier, NSInteger, NSNameSpecifier, NSNumber,
    NSCloneCommand, NSMoveCommand, NSScriptClassDescription, NSScriptCommand, NSScriptObjectSpecifier, NSString,
    NSUInteger, NSUniqueIDSpecifier, NSURL,
};
use serde_json::{json, Value as Json};
use tauri::{AppHandle, Manager};

use super::model::{self, ScriptValue, TrackEdit, TrackKey};
use super::{Bridge, ScriptError, ANSWER_TIMEOUT, EXPORT_TIMEOUT};
use crate::state::AppState;

/// The application, for the Objective-C entry points, which are handed
/// nothing but `self`.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Registers the classes and extends `NSApplication`. The dictionary names
/// the classes, and Cocoa looks them up by name when the first Apple Event
/// loads it; a class `define_class!` has not registered yet is not found.
pub fn install(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let _ = (
        RbxTrack::class(),
        RbxPlaylist::class(),
        RbxDeck::class(),
        RbxDevice::class(),
        RbxLinkPlayer::class(),
        RbxSetting::class(),
        RbxPlayCommand::class(),
        RbxPauseCommand::class(),
        RbxLoadCommand::class(),
        RbxAddCommand::class(),
        RbxRemoveCommand::class(),
        RbxExportCommand::class(),
    );
    extend_application();
}

// ------------------------------------------------------------ the backend

fn app() -> Result<&'static AppHandle, ScriptError> {
    APP.get().ok_or_else(|| ScriptError::failed("rbxport is still starting."))
}

fn state() -> Result<Arc<AppState>, ScriptError> {
    Ok(Arc::clone(app()?.state::<Arc<AppState>>().inner()))
}

fn library() -> Result<Arc<rbl_index::Library>, ScriptError> {
    Ok(state()?.library()?)
}

fn bridge() -> Result<Arc<Bridge>, ScriptError> {
    Ok(Arc::clone(app()?.state::<Arc<Bridge>>().inner()))
}

// ------------------------------------------------ suspending and resuming

/// A command held while the work it started runs, and what that work said.
struct Suspended {
    command: Retained<NSScriptCommand>,
    /// Pieces of work still out. `set rating of every track whose …` starts
    /// one per track, and the reply waits for the last.
    pending: usize,
    result: Option<ScriptValue>,
    error: Option<ScriptError>,
}

thread_local! {
    /// By the command's address. Main thread only: Cocoa runs every command
    /// there, and each piece of work finishes there.
    static SUSPENDED: RefCell<HashMap<usize, Suspended>> = RefCell::new(HashMap::new());
}

/// Runs `work` off the main thread and holds the running command's reply
/// until it, and anything else the same command started, is done. The last
/// result is the command's; the first error is.
fn defer<F>(work: F)
where
    F: Future<Output = Result<ScriptValue, ScriptError>> + Send + 'static,
{
    let Some(command) = NSScriptCommand::currentCommand() else {
        // Reached outside a command, which Cocoa does not do; do the work
        // anyway rather than drop it.
        tauri::async_runtime::spawn(async move {
            if let Err(e) = work.await {
                tracing::warn!(error = %e.message, "a script's change failed outside a command");
            }
        });
        return;
    };
    let app = match app() {
        Ok(app) => app.clone(),
        Err(e) => return fail(&e),
    };
    let key = Retained::as_ptr(&command).addr();
    SUSPENDED.with_borrow_mut(|held| {
        held.entry(key)
            .or_insert_with(|| {
                command.suspendExecution();
                Suspended { command: command.clone(), pending: 0, result: None, error: None }
            })
            .pending += 1;
        tracing::debug!(command = key, pending = held.get(&key).map_or(0, |s| s.pending), "script work started");
    });
    tauri::async_runtime::spawn(async move {
        let outcome = work.await;
        if let Err(e) = app.run_on_main_thread(move || finish(key, outcome)) {
            tracing::warn!(error = %e, "a script command could not be resumed");
        }
    });
}

fn finish(key: usize, outcome: Result<ScriptValue, ScriptError>) {
    let done = SUSPENDED.with_borrow_mut(|held| {
        let entry = held.get_mut(&key)?;
        entry.pending = entry.pending.saturating_sub(1);
        tracing::debug!(command = key, pending = entry.pending, "script work finished");
        match outcome {
            Ok(value) => entry.result = Some(value),
            Err(error) => {
                entry.error.get_or_insert(error);
            }
        }
        if entry.pending == 0 { held.remove(&key) } else { None }
    });
    let (Some(done), Some(mtm)) = (done, MainThreadMarker::new()) else { return };
    if let Some(error) = &done.error {
        set_error(&done.command, error);
    }
    let result = done.result.as_ref().and_then(|value| to_objc(value, mtm)).map(|object| {
        // An object goes back as its reference: resumed with the object
        // itself, `make` throws, since Cocoa only turns a result into a
        // reference on the way out of a command that was not suspended.
        // SAFETY: `objectSpecifier` takes nothing and returns a specifier
        // or nil; every scripting class here answers it.
        let specifier: Option<Retained<NSScriptObjectSpecifier>> = unsafe { msg_send![&*object, objectSpecifier] };
        let is_scripting_object = object.downcast_ref::<RbxTrack>().is_some() || object.downcast_ref::<RbxPlaylist>().is_some();
        match specifier {
            Some(specifier) if is_scripting_object => any(specifier),
            _ => object,
        }
    });
    let command = done.command;
    // SAFETY: the command was suspended by `defer` on this thread and is
    // resumed exactly once, when the last of its work is in. Called inside
    // `catch` because this runs in a run-loop callback, where an exception
    // Cocoa throws would otherwise abort the process.
    let resumed = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
        command.resumeExecutionWithResult(result.as_deref());
    }));
    if let Err(exception) = resumed {
        tracing::error!(exception = ?exception, "resuming a script command threw");
    }
}

fn set_error(command: &NSScriptCommand, error: &ScriptError) {
    command.setScriptErrorNumber(NSInteger::try_from(error.code).unwrap_or(-10_000));
    command.setScriptErrorString(Some(&NSString::from_str(&error.message)));
}

/// Fails the running command with `error`.
fn fail(error: &ScriptError) {
    if let Some(command) = NSScriptCommand::currentCommand() {
        set_error(&command, error);
    } else {
        tracing::warn!(error = %error.message, "a script error with no command to report it");
    }
}

/// A backend command's result as a script's: nothing to return.
fn done<T>(result: Result<T, crate::error::AppError>) -> Result<ScriptValue, ScriptError> {
    result.map(|_| ScriptValue::Missing).map_err(ScriptError::from)
}

// -------------------------------------------------------- value conversion

fn any<T: Message>(object: Retained<T>) -> Retained<AnyObject> {
    // SAFETY: every Objective-C object is an `AnyObject`.
    unsafe { Retained::cast_unchecked(object) }
}

fn to_objc(value: &ScriptValue, mtm: MainThreadMarker) -> Option<Retained<AnyObject>> {
    Some(match value {
        ScriptValue::Missing => return None,
        ScriptValue::Bool(b) => any(NSNumber::numberWithBool(*b)),
        ScriptValue::Int(n) => any(NSNumber::numberWithLongLong(*n)),
        ScriptValue::Real(n) => any(NSNumber::numberWithDouble(*n)),
        ScriptValue::Enum(code) => any(NSNumber::numberWithUnsignedInt(*code)),
        ScriptValue::Text(text) => any(NSString::from_str(text)),
        ScriptValue::File(path) => any(NSURL::from_file_path(path)?),
        ScriptValue::List(items) => {
            let items: Vec<Retained<AnyObject>> = items.iter().filter_map(|item| to_objc(item, mtm)).collect();
            any(NSArray::from_retained_slice(&items))
        }
        ScriptValue::Track { id, playlist } => any(RbxTrack::new(mtm, *id, *playlist)),
        ScriptValue::Playlist(id) => any(RbxPlaylist::new(mtm, *id)),
    })
}

/// A value as an Apple Event descriptor, for a property typed `any`.
fn descriptor(value: &ScriptValue) -> Retained<NSAppleEventDescriptor> {
    let class = NSAppleEventDescriptor::class();
    // SAFETY: each is a class method of NSAppleEventDescriptor taking the
    // argument type given and returning a new descriptor. Sent by message
    // because their bindings need Core Services' types.
    unsafe {
        match value {
            ScriptValue::Bool(b) => msg_send![class, descriptorWithBoolean: u8::from(*b)],
            ScriptValue::Int(n) => match i32::try_from(*n) {
                Ok(n) => msg_send![class, descriptorWithInt32: n],
                #[allow(clippy::cast_precision_loss, reason = "past i32 AppleScript holds it as a real anyway")]
                Err(_) => msg_send![class, descriptorWithDouble: *n as f64],
            },
            ScriptValue::Real(n) => msg_send![class, descriptorWithDouble: *n],
            ScriptValue::Text(text) => msg_send![class, descriptorWithString: &*NSString::from_str(text)],
            ScriptValue::List(items) => {
                let list: Retained<NSAppleEventDescriptor> = msg_send![class, listDescriptor];
                for (at, item) in items.iter().enumerate() {
                    let item = descriptor(item);
                    let index = NSInteger::try_from(at + 1).unwrap_or(NSInteger::MAX);
                    let _: () = msg_send![&*list, insertDescriptor: &*item, atIndex: index];
                }
                list
            }
            // `missing value`, which AppleScript carries as the type `msng`.
            _ => msg_send![class, descriptorWithTypeCode: model::four_cc(*b"msng")],
        }
    }
}

fn is_bool(number: &NSNumber) -> bool {
    number.class() == NSNumber::numberWithBool(true).class()
}

fn is_float(number: &NSNumber) -> bool {
    // SAFETY: `objCType` is a NUL-terminated encoding string owned by the number.
    let encoding = unsafe { CStr::from_ptr(number.objCType().as_ptr()) };
    matches!(encoding.to_bytes(), b"d" | b"f")
}

/// Whatever Cocoa handed a setter or a command, as a value.
fn from_objc(object: Option<&AnyObject>) -> ScriptValue {
    let Some(object) = object else { return ScriptValue::Missing };
    if let Some(number) = object.downcast_ref::<NSNumber>() {
        if is_bool(number) {
            ScriptValue::Bool(number.boolValue())
        } else if is_float(number) {
            ScriptValue::Real(number.doubleValue())
        } else {
            ScriptValue::Int(number.longLongValue())
        }
    } else if let Some(text) = object.downcast_ref::<NSString>() {
        ScriptValue::Text(text.to_string())
    } else if let Some(items) = object.downcast_ref::<NSArray>() {
        ScriptValue::List(items.iter().map(|item| from_objc(Some(&*item))).collect())
    } else if let Some(url) = object.downcast_ref::<NSURL>() {
        url.to_file_path().map_or(ScriptValue::Missing, ScriptValue::File)
    } else if let Some(descriptor) = object.downcast_ref::<NSAppleEventDescriptor>() {
        from_descriptor(descriptor)
    } else if let Some(track) = object.downcast_ref::<RbxTrack>() {
        ScriptValue::Track { id: track.ivars().id, playlist: track.ivars().playlist }
    } else if let Some(playlist) = object.downcast_ref::<RbxPlaylist>() {
        ScriptValue::Playlist(playlist.ivars().id.get())
    } else {
        ScriptValue::Missing
    }
}

/// An Apple Event value Cocoa passed through uncoerced, which it does for a
/// property typed `any`.
fn from_descriptor(descriptor: &NSAppleEventDescriptor) -> ScriptValue {
    // SAFETY: `descriptorType` takes nothing and returns a `DescType`, a
    // four-character code. Read by message because its binding needs Core
    // Services' types.
    let kind: u32 = unsafe { msg_send![descriptor, descriptorType] };
    let kind = kind.to_be_bytes();
    match &kind {
        b"true" => ScriptValue::Bool(true),
        b"fals" => ScriptValue::Bool(false),
        b"bool" => ScriptValue::Bool(descriptor.booleanValue() != 0),
        b"shor" | b"long" => ScriptValue::Int(i64::from(descriptor.int32Value())),
        b"comp" | b"doub" | b"sing" => ScriptValue::Real(descriptor.doubleValue()),
        b"list" => ScriptValue::List(
            (1..=descriptor.numberOfItems())
                .filter_map(|i| descriptor.descriptorAtIndex(i))
                .map(|item| from_descriptor(&item))
                .collect(),
        ),
        b"null" | b"msng" | b"type" => ScriptValue::Missing,
        _ => descriptor.stringValue().map_or(ScriptValue::Missing, |text| ScriptValue::Text(text.to_string())),
    }
}

/// A track or playlist id as a unique-id specifier carries it: our own text,
/// or a number when a script wrote `track id 42` without the quotes.
fn id_of(object: Option<&AnyObject>) -> Option<u64> {
    match from_objc(object) {
        ScriptValue::Text(text) => text.trim().parse().ok(),
        ScriptValue::Int(n) => u64::try_from(n).ok(),
        ScriptValue::Real(n) if n >= 0.0 && n.fract() == 0.0 => format!("{n:.0}").parse().ok(),
        _ => None,
    }
}

/// The objects a parameter names. The dictionary types the commands'
/// parameters as specifiers, so Cocoa hands them over unevaluated: typed as
/// its classes, a deck or playlist named in an argument was refused ("Can't
/// get deck 1") and one named as the direct parameter was sent the command
/// as a receiver instead of to the command's own class. Lists are
/// flattened: `every track whose …` is one specifier for many tracks, and
/// `{track 1, track 2}` a list of them.
fn resolve(object: Option<Retained<AnyObject>>) -> Vec<Retained<AnyObject>> {
    fn walk(object: Retained<AnyObject>, out: &mut Vec<Retained<AnyObject>>) {
        if let Some(specifier) = object.downcast_ref::<NSScriptObjectSpecifier>() {
            if let Some(found) = specifier.objectsByEvaluatingSpecifier() {
                walk(found, out);
            }
            return;
        }
        match object.downcast::<NSArray>() {
            Ok(items) => items.iter().for_each(|item| walk(item, out)),
            Err(object) => out.push(object),
        }
    }
    let mut out = Vec::new();
    if let Some(object) = object {
        walk(object, &mut out);
    }
    out
}

/// The command's direct parameter as objects: its receivers when Cocoa
/// made it one, as it does with any object named there.
fn direct(command: &NSScriptCommand) -> Vec<Retained<AnyObject>> {
    match command.evaluatedReceivers() {
        Some(receivers) => resolve(Some(receivers)),
        None => resolve(command.directParameter()),
    }
}

thread_local! {
    /// The commands already carried out in this pass, by address.
    static HANDLED: RefCell<std::collections::HashSet<usize>> = RefCell::new(std::collections::HashSet::new());
}

/// Carries out `command` once, however many of its receivers Cocoa asks.
///
/// An object named as a command's direct parameter is its receiver, and a
/// receiver's class has to say it handles the command (`responds-to` in the
/// dictionary) or Cocoa refuses it. With several — `add (every track whose
/// …) to …` — Cocoa calls each one's handler; the first does the whole
/// command for all of them, the rest do nothing.
fn once(command: &NSScriptCommand, run: impl FnOnce(&NSScriptCommand)) {
    let key = std::ptr::from_ref(command).addr();
    let first = HANDLED.with_borrow_mut(|handled| handled.insert(key));
    if !first {
        return;
    }
    // Forgotten once this pass over the run loop ends, after Cocoa has
    // called every receiver; an address can be a new command's next time.
    if let Ok(app) = app() {
        let _ = app.run_on_main_thread(move || {
            HANDLED.with_borrow_mut(|handled| handled.remove(&key));
        });
    }
    run(command);
}

/// Whether the command was given `key` at all, found or not.
fn given(command: &NSScriptCommand, key: &str) -> bool {
    command.arguments().is_some_and(|args| args.objectForKey(&NSString::from_str(key)).is_some())
}

fn argument(command: &NSScriptCommand, key: &str) -> Option<Retained<AnyObject>> {
    resolve(command.arguments().and_then(|args| args.objectForKey(&NSString::from_str(key)))).into_iter().next()
}

fn class_description(class: &AnyClass) -> Option<Retained<NSScriptClassDescription>> {
    // SAFETY: a class the dictionary describes, or NSApplication.
    unsafe { NSScriptClassDescription::classDescriptionForClass(class) }
}

fn application_description() -> Option<Retained<NSScriptClassDescription>> {
    class_description(NSApplication::class())
}

fn unique_id_specifier(
    container: Option<(&NSScriptClassDescription, &NSScriptObjectSpecifier)>,
    key: &str,
    id: &AnyObject,
) -> Option<Retained<NSScriptObjectSpecifier>> {
    let application = application_description()?;
    let (description, specifier) = container.map_or((&*application, None), |(d, s)| (d, Some(s)));
    // SAFETY: `id` is the object the key's elements answer `uniqueID` with.
    let specifier = unsafe {
        NSUniqueIDSpecifier::initWithContainerClassDescription_containerSpecifier_key_uniqueID(
            NSUniqueIDSpecifier::alloc(),
            description,
            specifier,
            &NSString::from_str(key),
            id,
        )
    };
    Some(specifier.into_super())
}

fn name_specifier(key: &str, name: &str) -> Option<Retained<NSScriptObjectSpecifier>> {
    let application = application_description()?;
    let specifier = NSNameSpecifier::initWithContainerClassDescription_containerSpecifier_key_name(
        NSNameSpecifier::alloc(),
        &application,
        None,
        &NSString::from_str(key),
        &NSString::from_str(name),
    );
    Some(specifier.into_super())
}

// ------------------------------------------------------------------ track

#[derive(Default)]
struct TrackIvars {
    id: u64,
    /// The playlist the track was reached through, if any.
    playlist: Option<u64>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RbxTrack"]
    #[ivars = TrackIvars]
    struct RbxTrack;

    impl RbxTrack {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Retained<Self> {
            let this = this.set_ivars(TrackIvars::default());
            // SAFETY: NSObject's designated initialiser.
            unsafe { msg_send![super(this), init] }
        }

        #[unsafe(method_id(valueForKey:))]
        fn value_for_key(&self, key: &NSString) -> Option<Retained<AnyObject>> {
            match TrackKey::parse(&key.to_string()) {
                Some(key) => {
                    let value = library().ok().and_then(|library| model::track_value(&library, self.ivars().id, key));
                    to_objc(&value.unwrap_or(ScriptValue::Missing), self.mtm())
                }
                // SAFETY: NSObject's own key-value coding.
                None => unsafe { msg_send![super(self), valueForKey: key] },
            }
        }

        #[unsafe(method(setValue:forKey:))]
        fn set_value_for_key(&self, value: Option<&AnyObject>, key: &NSString) {
            let Some(which) = TrackKey::parse(&key.to_string()) else {
                // SAFETY: NSObject's own key-value coding.
                return unsafe { msg_send![super(self), setValue: value, forKey: key] };
            };
            let mut value = from_objc(value);
            // An enumerator arrives as its code, a plain number.
            if let (TrackKey::Color, ScriptValue::Int(code)) = (which, &value) {
                value = u32::try_from(*code).map_or(ScriptValue::Missing, ScriptValue::Enum);
            }
            match model::track_edit(which, &value) {
                Ok(edit) => edit_track(self.ivars().id, edit),
                Err(e) => fail(&e),
            }
        }

        #[unsafe(method_id(objectSpecifier))]
        fn object_specifier(&self) -> Option<Retained<NSScriptObjectSpecifier>> {
            track_specifier(self.mtm(), self.ivars().id, self.ivars().playlist)
        }

        #[unsafe(method_id(scriptLoad:))]
        fn script_load(&self, command: &NSScriptCommand) -> Option<Retained<AnyObject>> {
            once(command, load);
            None
        }

        #[unsafe(method_id(scriptAdd:))]
        fn script_add(&self, command: &NSScriptCommand) -> Option<Retained<AnyObject>> {
            once(command, add_command);
            None
        }

        #[unsafe(method_id(scriptRemove:))]
        fn script_remove(&self, command: &NSScriptCommand) -> Option<Retained<AnyObject>> {
            once(command, remove_command);
            None
        }

        /// `duplicate track …` builds a new track from this one's
        /// properties before anything is inserted; refused here, where it
        /// starts, rather than with Cocoa's own puzzling message after.
        #[unsafe(method(setScriptingProperties:))]
        fn set_scripting_properties(&self, properties: Option<&AnyObject>) {
            if cloning() {
                return fail(&ScriptError::failed("A track cannot be duplicated. Use `add` to put it on a playlist."));
            }
            // SAFETY: NSObject's own scripting.
            unsafe { msg_send![super(self), setScriptingProperties: properties] }
        }
    }
);

/// `track id "…"`, or `track id "…" of playlist id "…"` for one reached
/// through a playlist.
fn track_specifier(mtm: MainThreadMarker, id: u64, playlist: Option<u64>) -> Option<Retained<NSScriptObjectSpecifier>> {
    let id = any(NSString::from_str(&id.to_string()));
    let Some(playlist) = playlist else { return unique_id_specifier(None, "rbxTracks", &id) };
    let container = RbxPlaylist::new(mtm, playlist);
    let description = class_description(RbxPlaylist::class())?;
    // SAFETY: `objectSpecifier` takes nothing and returns an object
    // specifier or nil.
    let specifier: Option<Retained<NSScriptObjectSpecifier>> = unsafe { msg_send![&*container, objectSpecifier] };
    unique_id_specifier(Some((&description, &*specifier?)), "rbxTracks", &id)
}

impl RbxTrack {
    fn new(mtm: MainThreadMarker, id: u64, playlist: Option<u64>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TrackIvars { id, playlist });
        // SAFETY: NSObject's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }
}

fn edit_track(id: u64, edit: TrackEdit) {
    let _ = write_now(async move {
        let app = app()?;
        let state = app.state::<Arc<AppState>>();
        let track = id.to_string();
        match edit {
            TrackEdit::Rating(stars) => done(crate::commands::set_track_rating(app.clone(), state, vec![track], stars).await),
            TrackEdit::Comment(comment) => {
                done(crate::commands::set_track_comment(app.clone(), state, vec![track], comment).await)
            }
            TrackEdit::Color(color) => {
                done(crate::commands::set_track_color(app.clone(), state, vec![track], Some(color)).await)
            }
            TrackEdit::Field(field, value) => {
                done(crate::details::set_track_field(app.clone(), state, vec![track], field.to_owned(), value).await)
            }
        }
    });
}

// --------------------------------------------------------------- playlist

struct PlaylistIvars {
    /// 0 for one `make` is still building, which has no id until it is
    /// written.
    id: Cell<u64>,
    /// What `make` gave it before it was written.
    name: RefCell<Option<String>>,
    kind: Cell<u32>,
}

impl Default for PlaylistIvars {
    fn default() -> Self {
        Self { id: Cell::new(0), name: RefCell::new(None), kind: Cell::new(model::KIND_PLAYLIST) }
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RbxPlaylist"]
    #[ivars = PlaylistIvars]
    struct RbxPlaylist;

    impl RbxPlaylist {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Retained<Self> {
            let this = this.set_ivars(PlaylistIvars::default());
            // SAFETY: NSObject's designated initialiser.
            unsafe { msg_send![super(this), init] }
        }

        #[unsafe(method_id(valueForKey:))]
        fn value_for_key(&self, key: &NSString) -> Option<Retained<AnyObject>> {
            let key_text = key.to_string();
            let id = self.ivars().id.get();
            let mtm = self.mtm();
            let value = match key_text.as_str() {
                "uniqueID" => ScriptValue::Text(id.to_string()),
                "name" if id == 0 => self.ivars().name.borrow().clone().map_or(ScriptValue::Missing, ScriptValue::Text),
                "rbxKind" if id == 0 => ScriptValue::Enum(self.ivars().kind.get()),
                "name" | "rbxKind" | "rbxParent" => {
                    match (library().ok().and_then(|library| model::playlist(&library, id)), key_text.as_str()) {
                        (None, _) => ScriptValue::Missing,
                        (Some(info), "name") => ScriptValue::Text(info.name),
                        (Some(info), "rbxKind") => ScriptValue::Enum(info.kind),
                        (Some(info), _) => info.parent.map_or(ScriptValue::Missing, ScriptValue::Playlist),
                    }
                }
                "rbxTracks" => ScriptValue::List(
                    library()
                        .map(|library| model::playlist_tracks(&library, id))
                        .unwrap_or_default()
                        .into_iter()
                        .map(|track| ScriptValue::Track { id: track, playlist: Some(id) })
                        .collect(),
                ),
                "rbxPlaylists" => ScriptValue::List(
                    library()
                        .map(|library| model::children(&library, Some(id)))
                        .unwrap_or_default()
                        .into_iter()
                        .map(ScriptValue::Playlist)
                        .collect(),
                ),
                // SAFETY: NSObject's own key-value coding.
                _ => return unsafe { msg_send![super(self), valueForKey: key] },
            };
            match value {
                // An empty list is still a list: `every track of` an empty
                // playlist is `{}`, not `missing value`.
                ScriptValue::List(items) if items.is_empty() => Some(any(NSArray::<AnyObject>::new())),
                value => to_objc(&value, mtm),
            }
        }

        #[unsafe(method(setValue:forKey:))]
        fn set_value_for_key(&self, value: Option<&AnyObject>, key: &NSString) {
            let id = self.ivars().id.get();
            match (key.to_string().as_str(), from_objc(value)) {
                ("name", ScriptValue::Text(name)) if id == 0 => *self.ivars().name.borrow_mut() = Some(name),
                ("name", ScriptValue::Text(name)) => drop(write_now(async move {
                    let app = app()?;
                    done(crate::commands::rename_playlist(app.clone(), app.state(), id.to_string(), name).await)
                })),
                ("name", _) => fail(&ScriptError::wrong_type("A playlist's name is text.")),
                ("rbxKind", ScriptValue::Int(code)) if id == 0 => {
                    self.ivars().kind.set(u32::try_from(code).unwrap_or(model::KIND_PLAYLIST));
                }
                ("rbxKind", _) => fail(&ScriptError::not_modifiable("A playlist's kind is set when it is made.")),
                // SAFETY: NSObject's own key-value coding.
                _ => unsafe { msg_send![super(self), setValue: value, forKey: key] },
            }
        }

        #[unsafe(method_id(objectSpecifier))]
        fn object_specifier(&self) -> Option<Retained<NSScriptObjectSpecifier>> {
            let id = any(NSString::from_str(&self.ivars().id.get().to_string()));
            unique_id_specifier(None, "rbxPlaylists", &id)
        }

        /// `track id "…" of playlist …` and `playlist id "…" of playlist …`,
        /// whether the id was written as text or as a number.
        #[unsafe(method_id(valueWithUniqueID:inPropertyWithKey:))]
        fn value_with_unique_id(&self, unique_id: &AnyObject, key: &NSString) -> Option<Retained<AnyObject>> {
            element_with_id(self.mtm(), self.ivars().id.get(), unique_id, &key.to_string())
        }

        /// `make new playlist at playlist "Folder"`: the end of the folder.
        #[unsafe(method(insertValue:inPropertyWithKey:))]
        fn insert_value(&self, value: &AnyObject, key: &NSString) {
            self.insert(value, key, None);
        }

        #[unsafe(method(insertValue:atIndex:inPropertyWithKey:))]
        fn insert_value_at(&self, value: &AnyObject, index: NSUInteger, key: &NSString) {
            self.insert(value, key, Some(index));
        }

        /// `delete track 3 of playlist "…"` takes it out of the playlist;
        /// `delete playlist 2 of playlist "Folder"` deletes that playlist.
        #[unsafe(method(removeValueAtIndex:fromPropertyWithKey:))]
        fn remove_value_at(&self, index: NSUInteger, key: &NSString) {
            // `move` takes the playlist out here and puts it in where it goes;
            // the insert does the whole move (see `insert_playlist`).
            if moving() {
                return;
            }
            let id = self.ivars().id.get();
            let Ok(library) = library() else { return fail(&ScriptError::failed("The library has not loaded yet.")) };
            match key.to_string().as_str() {
                "rbxTracks" => match model::playlist_tracks(&library, id).get(index) {
                    Some(&track) => drop(write_now(remove_work(id, vec![track]))),
                    None => fail(&ScriptError::no_such_object("That track is not on the playlist.")),
                },
                "rbxPlaylists" => match model::children(&library, Some(id)).get(index) {
                    Some(&child) => drop(write_now(delete_work(child))),
                    None => fail(&ScriptError::no_such_object("That playlist is not in the folder.")),
                },
                _ => fail(&ScriptError::not_modifiable("That cannot be deleted.")),
            }
        }

        #[unsafe(method_id(scriptExport:))]
        fn script_export(&self, command: &NSScriptCommand) -> Option<Retained<AnyObject>> {
            once(command, export);
            None
        }

        /// As a track's: `duplicate` is refused where it starts.
        #[unsafe(method(setScriptingProperties:))]
        fn set_scripting_properties(&self, properties: Option<&AnyObject>) {
            if cloning() {
                return fail(&ScriptError::failed("A playlist cannot be duplicated from a script."));
            }
            // SAFETY: NSObject's own scripting.
            unsafe { msg_send![super(self), setScriptingProperties: properties] }
        }
    }
);

/// An element of playlist `parent` by its id, if it is one.
fn element_with_id(mtm: MainThreadMarker, parent: u64, unique_id: &AnyObject, key: &str) -> Option<Retained<AnyObject>> {
    let wanted = id_of(Some(unique_id))?;
    let library = library().ok()?;
    match key {
        "rbxTracks" => model::playlist_tracks(&library, parent)
            .contains(&wanted)
            .then(|| any(RbxTrack::new(mtm, wanted, Some(parent)))),
        "rbxPlaylists" => {
            model::children(&library, Some(parent)).contains(&wanted).then(|| any(RbxPlaylist::new(mtm, wanted)))
        }
        _ => None,
    }
}

impl RbxPlaylist {
    fn new(mtm: MainThreadMarker, id: u64) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(PlaylistIvars { id: Cell::new(id), ..PlaylistIvars::default() });
        // SAFETY: NSObject's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }

    fn insert(&self, value: &AnyObject, key: &NSString, index: Option<NSUInteger>) {
        let id = self.ivars().id.get();
        if key.to_string() != "rbxPlaylists" {
            return fail(&ScriptError::failed("Use `add` to put tracks on a playlist."));
        }
        let kind = library().ok().and_then(|library| model::playlist(&library, id)).map(|info| info.kind);
        if kind != Some(model::KIND_FOLDER) {
            return fail(&ScriptError::failed("Playlists can only go inside a folder."));
        }
        insert_playlist(value, id.to_string(), index);
    }
}

/// Whether the command running is one of Cocoa's standard ones.
fn running<T: DowncastTarget>() -> bool {
    NSScriptCommand::currentCommand().is_some_and(|command| command.downcast_ref::<T>().is_some())
}

fn moving() -> bool {
    running::<NSMoveCommand>()
}

fn cloning() -> bool {
    running::<NSCloneCommand>()
}

/// A playlist going into a folder, or to the top for `ROOT`: one `make`
/// built, or an existing one `move` is moving.
///
/// Cocoa's `move` takes the object out of its old container and inserts it
/// into the new one. For a playlist that would be a delete and an empty new
/// one, so the removal does nothing while a move runs and the insert moves
/// the playlist instead.
fn insert_playlist(value: &AnyObject, parent: String, index: Option<NSUInteger>) {
    let Some(playlist) = value.downcast_ref::<RbxPlaylist>() else {
        return fail(&ScriptError::failed("Only playlists and folders go there."));
    };
    let existing = playlist.ivars().id.get();
    if existing != 0 {
        if !moving() {
            return fail(&ScriptError::failed("That playlist is already in the library."));
        }
        let _ = write_now(async move {
            let app = app()?;
            done(crate::commands::move_playlist(app.clone(), app.state(), existing.to_string(), parent, index).await)
        });
        return;
    }
    let kind = playlist.ivars().kind.get();
    if kind == model::KIND_SMART {
        return fail(&ScriptError::failed("A smart playlist's rule is made in the window. Make a regular playlist or a folder."));
    }
    let folder = kind == model::KIND_FOLDER;
    let name = playlist.ivars().name.borrow().clone().unwrap_or_else(|| {
        // rekordbox's own default names, as the window's menu uses them.
        if folder { "New folder" } else { "New playlist" }.to_owned()
    });
    let made = write_now(async move {
        let app = app()?;
        let written = Arc::new(StdMutex::new(None::<String>));
        let into = Arc::clone(&written);
        let under = parent.clone();
        crate::commands::edit(app.clone(), app.state(), "create_playlist", crate::commands::Touched::Playlists, move |w| {
            let id = if folder { w.create_folder(&name, &under)? } else { w.create_playlist(&name, &under)? };
            if let Ok(mut slot) = into.lock() {
                *slot = Some(id);
            }
            Ok(())
        })
        .await?;
        let id: u64 = written.lock().ok().and_then(|slot| slot.clone()).and_then(|id| id.parse().ok()).unwrap_or(0);
        if let Some(index) = index {
            crate::commands::move_playlist(app.clone(), app.state(), id.to_string(), parent, Some(index)).await?;
        }
        Ok(ScriptValue::Playlist(id))
    });
    // `make` answers with the object it built, which now names the playlist
    // it was written as.
    if let Some(ScriptValue::Playlist(id)) = made {
        playlist.ivars().id.set(id);
    }
}

async fn delete_work(id: u64) -> Result<ScriptValue, ScriptError> {
    let app = app()?;
    done(crate::commands::delete_playlist(app.clone(), app.state(), id.to_string()).await)
}

async fn add_work(playlist: u64, tracks: Vec<u64>) -> Result<ScriptValue, ScriptError> {
    let app = app()?;
    let tracks = tracks.iter().map(u64::to_string).collect();
    done(crate::commands::add_tracks_to_playlist(app.clone(), app.state(), playlist.to_string(), tracks).await)
}

async fn remove_work(playlist: u64, tracks: Vec<u64>) -> Result<ScriptValue, ScriptError> {
    let app = app()?;
    let tracks = tracks.iter().map(u64::to_string).collect();
    done(crate::commands::remove_tracks_from_playlist(app.clone(), app.state(), playlist.to_string(), tracks).await)
}

/// Makes a library change a key-value setter or container asked for, and
/// waits for it on the main thread.
///
/// Not suspended, as a command is: Cocoa calls a setter once per object in
/// `set rating of every track whose …`, and a command suspended inside one
/// of those calls was resumed before the rest had run — two of four ratings
/// were in when the reply went. Blocking here is safe because a library
/// write never waits on the main thread; asking the window would, which is
/// why the setters that do that suspend instead.
fn write_now<F>(work: F) -> Option<ScriptValue>
where
    F: Future<Output = Result<ScriptValue, ScriptError>> + Send + 'static,
{
    if let Err(e) = bridge().and_then(|bridge| bridge.refuse_if_protected()) {
        fail(&e);
        return None;
    }
    let (sender, outcome) = std::sync::mpsc::channel();
    tauri::async_runtime::spawn(async move {
        let _ = sender.send(work.await);
    });
    match outcome.recv() {
        Ok(Ok(value)) => Some(value),
        Ok(Err(e)) => {
            fail(&e);
            None
        }
        Err(_) => {
            fail(&ScriptError::failed("The change did not finish."));
            None
        }
    }
}

/// A command's library change, run while the command is suspended.
fn write_later<F>(work: F)
where
    F: Future<Output = Result<ScriptValue, ScriptError>> + Send + 'static,
{
    match bridge().and_then(|bridge| bridge.refuse_if_protected()) {
        Ok(()) => defer(work),
        Err(e) => fail(&e),
    }
}

// ------------------------------------------------------------------- deck

#[derive(Default)]
struct DeckIvars {
    /// 1 or 2: player A or player B.
    index: u8,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RbxDeck"]
    #[ivars = DeckIvars]
    struct RbxDeck;

    impl RbxDeck {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Retained<Self> {
            let this = this.set_ivars(DeckIvars::default());
            // SAFETY: NSObject's designated initialiser.
            unsafe { msg_send![super(this), init] }
        }

        #[unsafe(method_id(valueForKey:))]
        #[allow(clippy::cast_precision_loss, reason = "a frame count as seconds; exact below 2^53 frames")]
        fn value_for_key(&self, key: &NSString) -> Option<Retained<AnyObject>> {
            let index = self.ivars().index;
            let value = match key.to_string().as_str() {
                "rbxIndex" => ScriptValue::Int(i64::from(index)),
                "rbxCurrentTrack" => loaded_track(index)
                    .map_or(ScriptValue::Missing, |id| ScriptValue::Track { id, playlist: None }),
                key @ ("rbxPlaying" | "rbxPosition" | "rbxDuration" | "rbxTempo") => {
                    let deck = deck_snapshot(index);
                    let seconds = |frames: u64| {
                        deck.filter(|d| d.sample_rate > 0 && d.loaded)
                            .map_or(0.0, |d| frames as f64 / f64::from(d.sample_rate))
                    };
                    match key {
                        "rbxPlaying" => ScriptValue::Bool(deck.is_some_and(|d| d.playing)),
                        "rbxPosition" => ScriptValue::Real(seconds(deck.map_or(0, |d| d.position_frames))),
                        "rbxDuration" => ScriptValue::Real(seconds(deck.map_or(0, |d| d.total_frames))),
                        _ => ScriptValue::Real(deck.map_or(0.0, |d| (f64::from(d.tempo) - 1.0) * 100.0)),
                    }
                }
                // SAFETY: NSObject's own key-value coding.
                _ => return unsafe { msg_send![super(self), valueForKey: key] },
            };
            to_objc(&value, self.mtm())
        }

        #[unsafe(method_id(objectSpecifier))]
        fn object_specifier(&self) -> Option<Retained<NSScriptObjectSpecifier>> {
            deck_specifier(self.ivars().index)
        }

        #[unsafe(method_id(scriptPlay:))]
        fn script_play(&self, command: &NSScriptCommand) -> Option<Retained<AnyObject>> {
            once(command, |command| transport(command, "deck.play"));
            None
        }

        #[unsafe(method_id(scriptPause:))]
        fn script_pause(&self, command: &NSScriptCommand) -> Option<Retained<AnyObject>> {
            once(command, |command| transport(command, "deck.pause"));
            None
        }
    }
);

/// `deck 1` or `deck 2`.
fn deck_specifier(index: u8) -> Option<Retained<NSScriptObjectSpecifier>> {
    let application = application_description()?;
    let specifier = NSIndexSpecifier::initWithContainerClassDescription_containerSpecifier_key_index(
        NSIndexSpecifier::alloc(),
        &application,
        None,
        &NSString::from_str("rbxDecks"),
        NSInteger::from(index) - 1,
    );
    Some(specifier.into_super())
}

impl RbxDeck {
    fn new(mtm: MainThreadMarker, index: u8) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DeckIvars { index });
        // SAFETY: NSObject's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }
}

/// `a` or `b`, as the window names its decks.
fn deck_name(index: u8) -> &'static str {
    if index == 2 { "b" } else { "a" }
}

fn loaded_track(index: u8) -> Option<u64> {
    let player = app().ok()?.state::<Arc<crate::player::Player>>();
    let loaded = player.loaded_tracks.lock();
    loaded.get(&crate::player::deck_of(deck_name(index)))?.parse().ok()
}

fn deck_snapshot(index: u8) -> Option<rbl_deck::DeckSnapshot> {
    let player = app().ok()?.state::<Arc<crate::player::Player>>();
    let snapshot = player.opened()?.snapshot();
    Some(if index == 2 { snapshot.b } else { snapshot.a })
}

// ----------------------------------------------------------------- device

#[derive(Default)]
struct DeviceIvars {
    name: String,
    path: std::path::PathBuf,
    removable: bool,
    total: u64,
    free: u64,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RbxDevice"]
    #[ivars = DeviceIvars]
    struct RbxDevice;

    impl RbxDevice {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Retained<Self> {
            let this = this.set_ivars(DeviceIvars::default());
            // SAFETY: NSObject's designated initialiser.
            unsafe { msg_send![super(this), init] }
        }

        #[unsafe(method_id(valueForKey:))]
        #[allow(clippy::cast_precision_loss, reason = "a byte count as AppleScript's real; exact below 2^53")]
        fn value_for_key(&self, key: &NSString) -> Option<Retained<AnyObject>> {
            let ivars = self.ivars();
            let value = match key.to_string().as_str() {
                "name" => ScriptValue::Text(ivars.name.clone()),
                "rbxLocation" => ScriptValue::File(ivars.path.clone()),
                "rbxRemovable" => ScriptValue::Bool(ivars.removable),
                "rbxCapacity" => ScriptValue::Real(ivars.total as f64),
                "rbxFreeSpace" => ScriptValue::Real(ivars.free as f64),
                // SAFETY: NSObject's own key-value coding.
                _ => return unsafe { msg_send![super(self), valueForKey: key] },
            };
            to_objc(&value, self.mtm())
        }

        #[unsafe(method_id(objectSpecifier))]
        fn object_specifier(&self) -> Option<Retained<NSScriptObjectSpecifier>> {
            name_specifier("rbxDevices", &self.ivars().name)
        }
    }
);

impl RbxDevice {
    fn new(mtm: MainThreadMarker, device: &rbl_devices::Device) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DeviceIvars {
            name: device.name.clone(),
            path: device.mount_point.clone(),
            removable: device.removable,
            total: device.total_bytes,
            free: device.free_bytes,
        });
        // SAFETY: NSObject's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }
}

// ------------------------------------------------------------ link player

#[derive(Default)]
struct LinkPlayerIvars {
    number: u8,
    name: String,
    address: String,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RbxLinkPlayer"]
    #[ivars = LinkPlayerIvars]
    struct RbxLinkPlayer;

    impl RbxLinkPlayer {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Retained<Self> {
            let this = this.set_ivars(LinkPlayerIvars::default());
            // SAFETY: NSObject's designated initialiser.
            unsafe { msg_send![super(this), init] }
        }

        #[unsafe(method_id(valueForKey:))]
        fn value_for_key(&self, key: &NSString) -> Option<Retained<AnyObject>> {
            let ivars = self.ivars();
            let value = match key.to_string().as_str() {
                "uniqueID" => ScriptValue::Int(i64::from(ivars.number)),
                "name" => ScriptValue::Text(ivars.name.clone()),
                "rbxAddress" => ScriptValue::Text(ivars.address.clone()),
                // SAFETY: NSObject's own key-value coding.
                _ => return unsafe { msg_send![super(self), valueForKey: key] },
            };
            to_objc(&value, self.mtm())
        }

        #[unsafe(method_id(objectSpecifier))]
        fn object_specifier(&self) -> Option<Retained<NSScriptObjectSpecifier>> {
            unique_id_specifier(None, "rbxLinkPlayers", &any(NSNumber::numberWithUnsignedInt(u32::from(self.ivars().number))))
        }
    }
);

impl RbxLinkPlayer {
    fn new(mtm: MainThreadMarker, peer: crate::link::PeerDto) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(LinkPlayerIvars { number: peer.number, name: peer.name, address: peer.address });
        // SAFETY: NSObject's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }
}

// ---------------------------------------------------------------- setting

#[derive(Default)]
struct SettingIvars {
    /// `pane.field`, as `model::settings` names it.
    name: String,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RbxSetting"]
    #[ivars = SettingIvars]
    struct RbxSetting;

    impl RbxSetting {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Retained<Self> {
            let this = this.set_ivars(SettingIvars::default());
            // SAFETY: NSObject's designated initialiser.
            unsafe { msg_send![super(this), init] }
        }

        #[unsafe(method_id(valueForKey:))]
        fn value_for_key(&self, key: &NSString) -> Option<Retained<AnyObject>> {
            match key.to_string().as_str() {
                "name" => to_objc(&ScriptValue::Text(self.ivars().name.clone()), self.mtm()),
                // Handed over as an Apple Event value already: Cocoa fails a
                // property typed `any` with a number or a string in it
                // (-10000), where it passes a descriptor straight through.
                "rbxValue" => Some(any(descriptor(&setting_value_of(&self.ivars().name)))),
                // SAFETY: NSObject's own key-value coding.
                _ => unsafe { msg_send![super(self), valueForKey: key] },
            }
        }

        #[unsafe(method(setValue:forKey:))]
        fn set_value_for_key(&self, value: Option<&AnyObject>, key: &NSString) {
            if key.to_string() != "rbxValue" {
                // SAFETY: NSObject's own key-value coding.
                return unsafe { msg_send![super(self), setValue: value, forKey: key] };
            }
            let json = match model::setting_json(&from_objc(value)) {
                Ok(json) => json,
                Err(e) => return fail(&e),
            };
            let path = self.ivars().name.clone();
            ask_window("preferences.set", json!({ "path": path, "value": json }), |bridge, answer| {
                // The window answers with every preference as it now stands,
                // so the next read sees this change without waiting for the
                // window's own mirror to catch up.
                *bridge.preferences.write() = Some(answer);
                ScriptValue::Missing
            });
        }

        #[unsafe(method_id(objectSpecifier))]
        fn object_specifier(&self) -> Option<Retained<NSScriptObjectSpecifier>> {
            name_specifier("rbxSettings", &self.ivars().name)
        }
    }
);

impl RbxSetting {
    fn new(mtm: MainThreadMarker, name: String) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SettingIvars { name });
        // SAFETY: NSObject's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }
}

fn setting_value_of(name: &str) -> ScriptValue {
    current_settings().into_iter().find(|(n, _)| n == name).map_or(ScriptValue::Missing, |(_, value)| model::setting_value(&value))
}

fn current_settings() -> Vec<(String, Json)> {
    bridge().ok().and_then(|bridge| bridge.preferences()).map(|p| model::settings(&p)).unwrap_or_default()
}

/// Asks the window to do `action`, holding the command for its answer, which
/// `then` turns into the command's result.
fn ask_window<F>(action: &'static str, args: Json, then: F)
where
    F: FnOnce(&Bridge, Json) -> ScriptValue + Send + 'static,
{
    ask_window_for(action, args, ANSWER_TIMEOUT, then);
}

fn ask_window_for<F>(action: &'static str, args: Json, timeout: std::time::Duration, then: F)
where
    F: FnOnce(&Bridge, Json) -> ScriptValue + Send + 'static,
{
    defer(async move {
        let app = app()?.clone();
        let bridge = bridge()?;
        tauri::async_runtime::spawn_blocking(move || {
            let answer = bridge.ask(&app, action, args, timeout)?;
            Ok(then(&bridge, answer))
        })
        .await
        .map_err(|e| ScriptError::failed(format!("The request to the window failed: {e}")))?
    });
}

// --------------------------------------------------------------- commands

define_class!(
    // SAFETY: NSScriptCommand is meant to be subclassed, and this adds no ivars.
    #[unsafe(super(NSScriptCommand, NSObject))]
    #[name = "RbxPlayCommand"]
    struct RbxPlayCommand;

    impl RbxPlayCommand {
        #[unsafe(method_id(performDefaultImplementation))]
        fn perform(&self) -> Option<Retained<AnyObject>> {
            once(self, |command| transport(command, "deck.play"));
            None
        }
    }
);

define_class!(
    // SAFETY: NSScriptCommand is meant to be subclassed, and this adds no ivars.
    #[unsafe(super(NSScriptCommand, NSObject))]
    #[name = "RbxPauseCommand"]
    struct RbxPauseCommand;

    impl RbxPauseCommand {
        #[unsafe(method_id(performDefaultImplementation))]
        fn perform(&self) -> Option<Retained<AnyObject>> {
            once(self, |command| transport(command, "deck.pause"));
            None
        }
    }
);

/// PLAY or pause on the deck the command names, deck 1 when it names none.
fn transport(command: &NSScriptCommand, action: &'static str) {
    let named = command.directParameter().is_some();
    let deck = direct(command).first().and_then(|r| r.downcast_ref::<RbxDeck>().map(|d| d.ivars().index));
    let deck = match (named, deck) {
        (false, _) => 1,
        (true, Some(deck)) => deck,
        (true, None) => return fail(&ScriptError::no_such_object("There is no such deck: deck 1 or deck 2.")),
    };
    ask_window(action, json!({ "deck": deck_name(deck) }), |_, _| ScriptValue::Missing);
}

define_class!(
    // SAFETY: NSScriptCommand is meant to be subclassed, and this adds no ivars.
    #[unsafe(super(NSScriptCommand, NSObject))]
    #[name = "RbxLoadCommand"]
    struct RbxLoadCommand;

    impl RbxLoadCommand {
        #[unsafe(method_id(performDefaultImplementation))]
        fn perform(&self) -> Option<Retained<AnyObject>> {
            once(self, load);
            None
        }
    }
);

fn load(command: &NSScriptCommand) {
    let Some(track) = direct(command).first().and_then(|r| r.downcast_ref::<RbxTrack>().map(|t| t.ivars().id)) else {
        return fail(&ScriptError::no_such_object("Say which track to load: `load track 1 into deck 1`."));
    };
    let into = argument(command, "into");
    if given(command, "into") && into.is_none() {
        return fail(&ScriptError::no_such_object("There is no such deck or player."));
    }
    if let Some(player) = into.as_deref().and_then(|into| into.downcast_ref::<RbxLinkPlayer>()) {
        let number = player.ivars().number;
        return defer(async move {
            let state = state()?;
            let id = u32::try_from(track).map_err(|_| ScriptError::failed("That track's id is too large to send to a player."))?;
            state.link_load_track(number, id).map_err(ScriptError::failed)?;
            Ok(ScriptValue::Missing)
        });
    }
    let deck = into.as_deref().and_then(|into| into.downcast_ref::<RbxDeck>()).map_or(1, |deck| deck.ivars().index);
    let row = library().ok().and_then(|library| {
        let row = library.row_of_id(track)?;
        crate::state::rows_to_dto(&library, &[row], 0).into_iter().next()
    });
    let Some(row) = row else {
        return fail(&ScriptError::no_such_object("That track is not in the collection."));
    };
    ask_window("deck.load", json!({ "deck": deck_name(deck), "row": row }), |_, _| ScriptValue::Missing);
}

define_class!(
    // SAFETY: NSScriptCommand is meant to be subclassed, and this adds no ivars.
    #[unsafe(super(NSScriptCommand, NSObject))]
    #[name = "RbxAddCommand"]
    struct RbxAddCommand;

    impl RbxAddCommand {
        #[unsafe(method_id(performDefaultImplementation))]
        fn perform(&self) -> Option<Retained<AnyObject>> {
            once(self, add_command);
            None
        }
    }
);

define_class!(
    // SAFETY: NSScriptCommand is meant to be subclassed, and this adds no ivars.
    #[unsafe(super(NSScriptCommand, NSObject))]
    #[name = "RbxRemoveCommand"]
    struct RbxRemoveCommand;

    impl RbxRemoveCommand {
        #[unsafe(method_id(performDefaultImplementation))]
        fn perform(&self) -> Option<Retained<AnyObject>> {
            once(self, remove_command);
            None
        }
    }
);

fn add_command(command: &NSScriptCommand) {
    if let Some((playlist, tracks)) = tracks_and_playlist(command, "to") {
        write_later(add_work(playlist, tracks));
    }
}

fn remove_command(command: &NSScriptCommand) {
    if let Some((playlist, tracks)) = tracks_and_playlist(command, "from") {
        write_later(remove_work(playlist, tracks));
    }
}

/// The tracks `add` or `remove` names, and the regular playlist they go on
/// or come off; fails the command and returns `None` when either is wrong.
fn tracks_and_playlist(command: &NSScriptCommand, key: &str) -> Option<(u64, Vec<u64>)> {
    let tracks: Vec<u64> =
        direct(command).iter().filter_map(|r| r.downcast_ref::<RbxTrack>().map(|t| t.ivars().id)).collect();
    if tracks.is_empty() {
        fail(&ScriptError::missing_parameter("Say which tracks."));
        return None;
    }
    let Some(playlist) = argument(command, key).and_then(|p| p.downcast_ref::<RbxPlaylist>().map(|p| p.ivars().id.get()))
    else {
        fail(&ScriptError::no_such_object(format!("There is no such playlist: `{key} playlist \"…\"`.")));
        return None;
    };
    let kind = library().ok().and_then(|library| model::playlist(&library, playlist)).map(|info| info.kind);
    if kind != Some(model::KIND_PLAYLIST) {
        fail(&ScriptError::failed("Tracks can only be added to or removed from a regular playlist."));
        return None;
    }
    Some((playlist, tracks))
}

define_class!(
    // SAFETY: NSScriptCommand is meant to be subclassed, and this adds no ivars.
    #[unsafe(super(NSScriptCommand, NSObject))]
    #[name = "RbxExportCommand"]
    struct RbxExportCommand;

    impl RbxExportCommand {
        #[unsafe(method_id(performDefaultImplementation))]
        fn perform(&self) -> Option<Retained<AnyObject>> {
            once(self, export);
            None
        }
    }
);

fn export(command: &NSScriptCommand) {
    let Some(playlist) =
        direct(command).first().and_then(|r| r.downcast_ref::<RbxPlaylist>().map(|p| p.ivars().id.get()))
    else {
        return fail(&ScriptError::missing_parameter("Say which playlist to export."));
    };
    let Some(device) = argument(command, "to") else {
        return fail(&ScriptError::no_such_object("Say which device: `to device \"…\"`."));
    };
    let Some(device) = device.downcast_ref::<RbxDevice>() else {
        return fail(&ScriptError::wrong_type("A playlist is exported to a device."));
    };
    let args = json!({
        "playlist": playlist.to_string(),
        "device": { "name": device.ivars().name, "path": device.ivars().path.to_string_lossy() },
    });
    ask_window_for("export", args, EXPORT_TIMEOUT, |_, answer| {
        answer.as_str().map_or(ScriptValue::Missing, |said| ScriptValue::Text(said.to_owned()))
    });
}

// ------------------------------------------------------- the application

/// Adds the dictionary's application keys to `NSApplication`.
fn extend_application() {
    type Getter = extern "C-unwind" fn(&AnyObject, Sel) -> *mut AnyObject;
    type Lookup = extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) -> *mut AnyObject;
    type Setter = extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject);
    type Insert = extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject);
    type InsertAt = extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject, NSUInteger);
    type Remove = extern "C-unwind" fn(&AnyObject, Sel, NSUInteger);

    let class = NSApplication::class();
    // SAFETY: each function's signature matches the encoding it is added
    // with; `NSApplication` has none of these selectors of its own.
    unsafe {
        add(class, sel!(rbxTracks), transmute_getter(app_tracks as Getter), c"@@:");
        add(class, sel!(valueInRbxTracksWithUniqueID:), transmute_lookup(app_track_with_id as Lookup), c"@@:@");
        add(class, sel!(insertInRbxTracks:), transmute_insert(app_refuse_track as Insert), c"v@:@");
        add(class, sel!(insertObject:inRbxTracksAtIndex:), transmute_insert_at(app_refuse_track_at as InsertAt), c"v@:@Q");
        add(class, sel!(removeObjectFromRbxTracksAtIndex:), transmute_remove(app_refuse_remove_track as Remove), c"v@:Q");
        add(class, sel!(rbxPlaylists), transmute_getter(app_playlists as Getter), c"@@:");
        add(class, sel!(valueInRbxPlaylistsWithUniqueID:), transmute_lookup(app_playlist_with_id as Lookup), c"@@:@");
        add(class, sel!(insertInRbxPlaylists:), transmute_insert(app_insert_playlist as Insert), c"v@:@");
        add(class, sel!(insertObject:inRbxPlaylistsAtIndex:), transmute_insert_at(app_insert_playlist_at as InsertAt), c"v@:@Q");
        add(class, sel!(removeObjectFromRbxPlaylistsAtIndex:), transmute_remove(app_remove_playlist as Remove), c"v@:Q");
        add(class, sel!(rbxDecks), transmute_getter(app_decks as Getter), c"@@:");
        add(class, sel!(rbxDevices), transmute_getter(app_devices as Getter), c"@@:");
        add(class, sel!(rbxLinkPlayers), transmute_getter(app_link_players as Getter), c"@@:");
        add(class, sel!(valueInRbxLinkPlayersWithUniqueID:), transmute_lookup(app_link_player_with_id as Lookup), c"@@:@");
        add(class, sel!(rbxSettings), transmute_getter(app_settings as Getter), c"@@:");
        add(class, sel!(rbxLinkExport), transmute_getter(app_link_export as Getter), c"@@:");
        add(class, sel!(setRbxLinkExport:), transmute_setter(app_set_link_export as Setter), c"v@:@");
        add(class, sel!(rbxRekordboxRunning), transmute_getter(app_rekordbox_running as Getter), c"@@:");
    }
}

// Each is one function-pointer type to `Imp`, which the runtime calls with
// the arguments the encoding names.
fn transmute_getter(f: extern "C-unwind" fn(&AnyObject, Sel) -> *mut AnyObject) -> Imp {
    // SAFETY: a function pointer to a function pointer of the same size;
    // the runtime calls it with the signature the encoding gives.
    unsafe { std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel) -> *mut AnyObject, Imp>(f) }
}
fn transmute_lookup(f: extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) -> *mut AnyObject) -> Imp {
    // SAFETY: as `transmute_getter`.
    unsafe { std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) -> *mut AnyObject, Imp>(f) }
}
fn transmute_setter(f: extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject)) -> Imp {
    // SAFETY: as `transmute_getter`.
    unsafe { std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject), Imp>(f) }
}
fn transmute_insert(f: extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject)) -> Imp {
    transmute_setter(f)
}
fn transmute_insert_at(f: extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject, NSUInteger)) -> Imp {
    // SAFETY: as `transmute_getter`.
    unsafe { std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject, NSUInteger), Imp>(f) }
}
fn transmute_remove(f: extern "C-unwind" fn(&AnyObject, Sel, NSUInteger)) -> Imp {
    // SAFETY: as `transmute_getter`.
    unsafe { std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, NSUInteger), Imp>(f) }
}

/// # Safety
/// `imp` must take the arguments `types` encodes.
unsafe fn add(class: &AnyClass, sel: Sel, imp: Imp, types: &CStr) {
    // SAFETY: the caller's; the class pointer is a live class.
    let added = unsafe { objc2::ffi::class_addMethod(std::ptr::from_ref(class).cast_mut(), sel, imp, types.as_ptr()) };
    if !added.as_bool() {
        tracing::error!(selector = %sel.name().to_string_lossy(), "AppleScript could not extend NSApplication");
    }
}

/// An object returned to Objective-C at +0, as a getter's result is.
fn returned(object: Option<Retained<AnyObject>>) -> *mut AnyObject {
    object.map_or(std::ptr::null_mut(), Retained::autorelease_return)
}

fn list(values: &[ScriptValue]) -> *mut AnyObject {
    let Some(mtm) = MainThreadMarker::new() else { return std::ptr::null_mut() };
    let items: Vec<Retained<AnyObject>> = values.iter().filter_map(|value| to_objc(value, mtm)).collect();
    returned(Some(any(NSArray::from_retained_slice(&items))))
}

extern "C-unwind" fn app_tracks(_: &AnyObject, _: Sel) -> *mut AnyObject {
    let ids = library().map(|library| model::all_tracks(&library)).unwrap_or_default();
    list(&ids.into_iter().map(|id| ScriptValue::Track { id, playlist: None }).collect::<Vec<_>>())
}

extern "C-unwind" fn app_track_with_id(_: &AnyObject, _: Sel, id: *mut AnyObject) -> *mut AnyObject {
    // SAFETY: Cocoa passes a live object or nil.
    let id = id_of(unsafe { id.as_ref() });
    let found = id.filter(|&id| library().is_ok_and(|library| library.row_of_id(id).is_some()));
    let Some(mtm) = MainThreadMarker::new() else { return std::ptr::null_mut() };
    returned(found.map(|id| any(RbxTrack::new(mtm, id, None))))
}

extern "C-unwind" fn app_refuse_track(_: &AnyObject, _: Sel, _: *mut AnyObject) {
    fail(&ScriptError::failed("Tracks come into the collection by importing files in the window."));
}

extern "C-unwind" fn app_refuse_track_at(this: &AnyObject, sel: Sel, value: *mut AnyObject, _: NSUInteger) {
    app_refuse_track(this, sel, value);
}

extern "C-unwind" fn app_refuse_remove_track(_: &AnyObject, _: Sel, _: NSUInteger) {
    fail(&ScriptError::failed(
        "A track cannot be removed from the collection by a script. Delete it from a playlist to take it off that playlist.",
    ));
}

extern "C-unwind" fn app_playlists(_: &AnyObject, _: Sel) -> *mut AnyObject {
    let ids = library().map(|library| model::all_playlists(&library)).unwrap_or_default();
    list(&ids.into_iter().map(ScriptValue::Playlist).collect::<Vec<_>>())
}

extern "C-unwind" fn app_playlist_with_id(_: &AnyObject, _: Sel, id: *mut AnyObject) -> *mut AnyObject {
    // SAFETY: Cocoa passes a live object or nil.
    let id = id_of(unsafe { id.as_ref() });
    let found = id.filter(|&id| library().is_ok_and(|library| model::playlist(&library, id).is_some()));
    let Some(mtm) = MainThreadMarker::new() else { return std::ptr::null_mut() };
    returned(found.map(|id| any(RbxPlaylist::new(mtm, id))))
}

extern "C-unwind" fn app_insert_playlist(_: &AnyObject, _: Sel, value: *mut AnyObject) {
    // SAFETY: Cocoa passes the live object `make` built.
    match unsafe { value.as_ref() } {
        Some(value) => insert_playlist(value, rbl_db::write::ROOT.to_owned(), None),
        None => fail(&ScriptError::failed("Nothing to make.")),
    }
}

extern "C-unwind" fn app_insert_playlist_at(this: &AnyObject, sel: Sel, value: *mut AnyObject, _: NSUInteger) {
    // The application's playlists are every playlist at any depth, so an
    // index into them says nothing about where at the top it goes.
    app_insert_playlist(this, sel, value);
}

extern "C-unwind" fn app_remove_playlist(_: &AnyObject, _: Sel, index: NSUInteger) {
    // A move's removal; the insert that follows moves it (`insert_playlist`).
    if moving() {
        return;
    }
    let id = library().ok().and_then(|library| model::all_playlists(&library).get(index).copied());
    match id {
        Some(id) => drop(write_now(delete_work(id))),
        None => fail(&ScriptError::no_such_object("There is no such playlist.")),
    }
}

extern "C-unwind" fn app_decks(_: &AnyObject, _: Sel) -> *mut AnyObject {
    let Some(mtm) = MainThreadMarker::new() else { return std::ptr::null_mut() };
    let decks = [any(RbxDeck::new(mtm, 1)), any(RbxDeck::new(mtm, 2))];
    returned(Some(any(NSArray::from_retained_slice(&decks))))
}

extern "C-unwind" fn app_devices(_: &AnyObject, _: Sel) -> *mut AnyObject {
    let Some(mtm) = MainThreadMarker::new() else { return std::ptr::null_mut() };
    let devices = bridge().map(|bridge| bridge.devices()).unwrap_or_default();
    let devices: Vec<Retained<AnyObject>> = devices.iter().map(|device| any(RbxDevice::new(mtm, device))).collect();
    returned(Some(any(NSArray::from_retained_slice(&devices))))
}

/// The players heard on the network; mixers and other rekordboxes are not
/// something a track can be loaded onto.
fn link_players() -> Vec<crate::link::PeerDto> {
    state().map(|state| crate::link::peers(&state)).unwrap_or_default().into_iter().filter(|peer| peer.kind == "player").collect()
}

extern "C-unwind" fn app_link_players(_: &AnyObject, _: Sel) -> *mut AnyObject {
    let Some(mtm) = MainThreadMarker::new() else { return std::ptr::null_mut() };
    let players: Vec<Retained<AnyObject>> = link_players().into_iter().map(|peer| any(RbxLinkPlayer::new(mtm, peer))).collect();
    returned(Some(any(NSArray::from_retained_slice(&players))))
}

extern "C-unwind" fn app_link_player_with_id(_: &AnyObject, _: Sel, id: *mut AnyObject) -> *mut AnyObject {
    // SAFETY: Cocoa passes a live object or nil.
    let number = id_of(unsafe { id.as_ref() });
    let Some(mtm) = MainThreadMarker::new() else { return std::ptr::null_mut() };
    let found = link_players().into_iter().find(|peer| Some(u64::from(peer.number)) == number);
    returned(found.map(|peer| any(RbxLinkPlayer::new(mtm, peer))))
}

extern "C-unwind" fn app_settings(_: &AnyObject, _: Sel) -> *mut AnyObject {
    let Some(mtm) = MainThreadMarker::new() else { return std::ptr::null_mut() };
    let settings: Vec<Retained<AnyObject>> =
        current_settings().into_iter().map(|(name, _)| any(RbxSetting::new(mtm, name))).collect();
    returned(Some(any(NSArray::from_retained_slice(&settings))))
}

extern "C-unwind" fn app_link_export(_: &AnyObject, _: Sel) -> *mut AnyObject {
    let on = state().is_ok_and(|state| state.link_status().is_some());
    returned(Some(any(NSNumber::numberWithBool(on))))
}

extern "C-unwind" fn app_set_link_export(_: &AnyObject, _: Sel, value: *mut AnyObject) {
    // SAFETY: Cocoa passes a live object or nil.
    let ScriptValue::Bool(on) = from_objc(unsafe { value.as_ref() }) else {
        return fail(&ScriptError::wrong_type("Set link export to true or false."));
    };
    ask_window("link.set", json!({ "on": on }), |_, _| ScriptValue::Missing);
}

extern "C-unwind" fn app_rekordbox_running(_: &AnyObject, _: Sel) -> *mut AnyObject {
    returned(Some(any(NSNumber::numberWithBool(rbl_db::is_rekordbox_running()))))
}
