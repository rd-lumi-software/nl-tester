use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::path::{Component, Path, PathBuf};
#[cfg(windows)]
use std::sync::Arc;
use serde_json::Value;
use thiserror::Error;

const RAW_OUTPUT_DIRECTORY: &str = "raw";
const NORMALIZED_OUTPUT_DIRECTORY: &str = "normalized";

#[derive(Debug, Error)]
pub enum EvidenceBoundaryError {
    #[error("race-resistant evidence writes are unavailable on this platform")]
    UnsupportedPlatform,
    #[error("evidence output path '{path}' must contain only relative components")]
    InvalidRelativePath { path: String },
    #[error("evidence output path component '{component}' is invalid")]
    InvalidComponent { component: String },
    #[error("evidence boundary I/O failed: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceKind {
    HttpRequest,
    HttpResponse,
    WebsocketFrame,
    NormalizedState,
    NormalizedEvent,
}

impl EvidenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HttpRequest => "http_request",
            Self::HttpResponse => "http_response",
            Self::WebsocketFrame => "ws_frame",
            Self::NormalizedState => "normalized_state",
            Self::NormalizedEvent => "normalized_event",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceConfig {
    pub max_raw_bytes: usize,
    pub max_websocket_frames: usize,
    pub redact_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EvidenceRecord {
    pub kind: String,
    pub payload: String,
    pub bytes: usize,
    pub redacted: bool,
    pub truncated: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EvidenceCaptureError {
    #[error("evidence maxRawBytes must be greater than zero")]
    InvalidRawLimit,
    #[error("evidence maxWebSocketFrames must be greater than zero")]
    InvalidFrameLimit,
    #[error("evidence redaction field '{0}' is invalid")]
    InvalidRedactionField(String),
    #[error("evidence WebSocket frame limit exceeded")]
    FrameLimitExceeded,
    #[error("evidence payload could not be serialized: {0}")]
    Serialization(String),
}

/// Bounded in-memory evidence collector. Artifact writers are deliberately
/// separate: this object only prepares redacted bounded payloads and metadata.
#[derive(Debug, Clone)]
pub struct EvidenceCollector {
    config: EvidenceConfig,
    websocket_frames: usize,
    records: Vec<EvidenceRecord>,
}

impl EvidenceCollector {
    pub fn new(config: EvidenceConfig) -> Result<Self, EvidenceCaptureError> {
        if config.max_raw_bytes == 0 {
            return Err(EvidenceCaptureError::InvalidRawLimit);
        }
        if config.max_websocket_frames == 0 {
            return Err(EvidenceCaptureError::InvalidFrameLimit);
        }
        let mut fields = Vec::new();
        for field in config.redact_fields {
            let field = field.trim().to_ascii_lowercase();
            if field.is_empty() {
                return Err(EvidenceCaptureError::InvalidRedactionField(field));
            }
            if !fields.contains(&field) {
                fields.push(field);
            }
        }
        Ok(Self {
            config: EvidenceConfig {
                max_raw_bytes: config.max_raw_bytes,
                max_websocket_frames: config.max_websocket_frames,
                redact_fields: fields,
            },
            websocket_frames: 0,
            records: Vec::new(),
        })
    }

    pub fn records(&self) -> &[EvidenceRecord] {
        &self.records
    }

    pub fn capture_json(
        &mut self,
        kind: EvidenceKind,
        value: &Value,
    ) -> Result<EvidenceRecord, EvidenceCaptureError> {
        if kind == EvidenceKind::WebsocketFrame {
            if self.websocket_frames >= self.config.max_websocket_frames {
                return Err(EvidenceCaptureError::FrameLimitExceeded);
            }
            self.websocket_frames += 1;
        }
        let mut redacted_value = value.clone();
        let redacted = redact_value(&mut redacted_value, &self.config.redact_fields);
        let serialized = serde_json::to_vec(&redacted_value)
            .map_err(|error| EvidenceCaptureError::Serialization(error.to_string()))?;
        let bytes = serialized.len();
        let truncated = bytes > self.config.max_raw_bytes;
        let payload = if truncated {
            String::from_utf8_lossy(&serialized[..self.config.max_raw_bytes]).into_owned()
        } else {
            String::from_utf8(serialized)
                .map_err(|error| EvidenceCaptureError::Serialization(error.to_string()))?
        };
        let record = EvidenceRecord {
            kind: kind.as_str().to_string(),
            payload,
            bytes,
            redacted,
            truncated,
        };
        self.records.push(record.clone());
        Ok(record)
    }
}

fn redact_value(value: &mut Value, fields: &[String]) -> bool {
    match value {
        Value::Object(object) => {
            let mut changed = false;
            for (key, child) in object.iter_mut() {
                if fields.iter().any(|field| field == &key.to_ascii_lowercase()) {
                    *child = Value::String("[REDACTED]".to_string());
                    changed = true;
                } else if redact_value(child, fields) {
                    changed = true;
                }
            }
            changed
        }
        Value::Array(values) => values.iter_mut().any(|child| redact_value(child, fields)),
        _ => false,
    }
}

/// A trusted flow-root authority established from the lexical flow path.
#[derive(Debug, Clone)]
pub(crate) struct FlowRootAuthority {
    canonical_path: PathBuf,
    #[cfg(windows)]
    handle: Arc<File>,
}

impl FlowRootAuthority {
    pub(crate) fn establish(path: &Path) -> Result<Self, EvidenceBoundaryError> {
        #[cfg(windows)]
        {
            let (handle, canonical_path) = open_directory_no_follow(path)?;
            return Ok(Self {
                canonical_path,
                handle: Arc::new(handle),
            });
        }

        #[cfg(not(windows))]
        {
            Ok(Self {
                canonical_path: std::fs::canonicalize(path)?,
            })
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.canonical_path
    }
}

/// A flow-relative evidence root that retains the trusted flow-root authority
/// for the entire lifetime of its writers.
#[derive(Debug, Clone)]
pub struct EvidenceRoot {
    relative_components: Vec<OsString>,
    resolved_path: PathBuf,
    #[cfg(windows)]
    flow_root_authority: Arc<File>,
}

impl EvidenceRoot {
    pub(crate) fn new(
        authority: FlowRootAuthority,
        relative_components: Vec<OsString>,
        resolved_path: PathBuf,
    ) -> Self {
        Self {
            relative_components,
            resolved_path,
            #[cfg(windows)]
            flow_root_authority: authority.handle,
        }
    }

    pub fn path(&self) -> &Path {
        &self.resolved_path
    }

    /// Opens raw/normalized directories relative to the retained flow-root
    /// authority.
    ///
    /// Windows uses `NtCreateFile` with a directory handle as
    /// `RootDirectory`, `OBJ_DONT_REPARSE`, and `FILE_OPEN_REPARSE_POINT` for
    /// every component. Other platforms return a structured error until an
    /// equivalent no-follow primitive is implemented; they never fall back to
    /// ordinary path-based writes.
    pub fn open_writer(&self) -> Result<EvidenceWriter, EvidenceBoundaryError> {
        open_writer(self)
    }
}

#[derive(Debug)]
pub struct EvidenceWriter {
    #[cfg(windows)]
    root: Arc<File>,
}

impl EvidenceWriter {
    pub fn create_raw_file(
        &self,
        relative_path: impl AsRef<Path>,
    ) -> Result<File, EvidenceBoundaryError> {
        self.create_output_file(RAW_OUTPUT_DIRECTORY, relative_path.as_ref())
    }

    pub fn create_normalized_file(
        &self,
        relative_path: impl AsRef<Path>,
    ) -> Result<File, EvidenceBoundaryError> {
        self.create_output_file(NORMALIZED_OUTPUT_DIRECTORY, relative_path.as_ref())
    }

    #[cfg(windows)]
    fn create_output_file(
        &self,
        category: &str,
        relative_path: &Path,
    ) -> Result<File, EvidenceBoundaryError> {
        let mut components = validate_relative_components(relative_path)?;
        let filename = components
            .pop()
            .ok_or_else(|| invalid_relative_path(relative_path))?;

        let mut directory = self.root.as_ref().try_clone()?;
        directory = open_relative_directory(&directory, OsString::from(category), true)?;
        for component in components {
            directory = open_relative_directory(&directory, component, true)?;
        }

        create_file_no_follow(&directory, &filename).map_err(EvidenceBoundaryError::from)
    }

    #[cfg(not(windows))]
    fn create_output_file(
        &self,
        _category: &str,
        relative_path: &Path,
    ) -> Result<File, EvidenceBoundaryError> {
        validate_relative_components(relative_path)?;
        Err(EvidenceBoundaryError::UnsupportedPlatform)
    }
}

fn validate_relative_components(path: &Path) -> Result<Vec<OsString>, EvidenceBoundaryError> {
    if path.is_absolute() {
        return Err(invalid_relative_path(path));
    }

    let mut components = Vec::new();
    for component in path.components() {
        let Component::Normal(value) = component else {
            if matches!(component, Component::CurDir) {
                continue;
            }
            return Err(invalid_relative_path(path));
        };

        let value = value.to_str().ok_or_else(|| invalid_relative_path(path))?;
        if value.is_empty()
            || value == "."
            || value == ".."
            || value.contains('\0')
            || (cfg!(windows) && value.contains(':'))
        {
            return Err(EvidenceBoundaryError::InvalidComponent {
                component: value.to_string(),
            });
        }
        components.push(value.to_owned().into());
    }

    if components.is_empty() {
        return Err(invalid_relative_path(path));
    }
    Ok(components)
}

fn invalid_relative_path(path: &Path) -> EvidenceBoundaryError {
    EvidenceBoundaryError::InvalidRelativePath {
        path: path.to_string_lossy().into_owned(),
    }
}

#[cfg(windows)]
fn open_writer(root: &EvidenceRoot) -> Result<EvidenceWriter, EvidenceBoundaryError> {
    let evidence_handle =
        open_relative_components(&root.flow_root_authority, &root.relative_components, true)?;
    Ok(EvidenceWriter {
        root: Arc::new(evidence_handle),
    })
}

#[cfg(not(windows))]
fn open_writer(_root: &EvidenceRoot) -> Result<EvidenceWriter, EvidenceBoundaryError> {
    Err(EvidenceBoundaryError::UnsupportedPlatform)
}

#[cfg(windows)]
fn open_relative_components(
    parent: &File,
    components: &[OsString],
    create_missing: bool,
) -> Result<File, EvidenceBoundaryError> {
    let mut current = parent.try_clone()?;
    for component in components {
        current = open_relative_directory(&current, component.clone(), create_missing)?;
    }
    Ok(current)
}

#[cfg(windows)]
fn open_relative_directory(
    parent: &File,
    component: OsString,
    create_missing: bool,
) -> Result<File, EvidenceBoundaryError> {
    let disposition = if create_missing {
        windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_IF
    } else {
        windows_sys::Wdk::Storage::FileSystem::FILE_OPEN
    };
    Ok(nt_create_file(
        Some(parent),
        &component,
        windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_READ
            | windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_WRITE
            | windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE,
        disposition,
        windows_sys::Wdk::Storage::FileSystem::FILE_DIRECTORY_FILE
            | windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_REPARSE_POINT
            | windows_sys::Wdk::Storage::FileSystem::FILE_SYNCHRONOUS_IO_NONALERT,
    )?)
}

#[cfg(windows)]
fn open_directory_no_follow(path: &Path) -> Result<(File, PathBuf), EvidenceBoundaryError> {
    use std::mem::size_of;
    use std::os::windows::io::FromRawHandle;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FileAttributeTagInfo, GetFileInformationByHandleEx, GetFinalPathNameByHandleW,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_NAME_NORMALIZED,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, VOLUME_NAME_DOS,
    };

    let name = lexically_normalize_windows_path(&win32_absolute_path(path)?);
    let mut wide = name.encode_utf16().collect::<Vec<_>>();
    wide.push(0);
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::from_raw_os_error(unsafe { GetLastError() as i32 }).into());
    }

    let mut tag_info = FILE_ATTRIBUTE_TAG_INFO {
        FileAttributes: 0,
        ReparseTag: 0,
    };
    let tag_ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileAttributeTagInfo,
            &mut tag_info as *mut _ as *mut _,
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if tag_ok == 0 || tag_info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        unsafe {
            CloseHandle(handle);
        }
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "flow evidence root is a reparse point",
        )
        .into());
    }

    let mut final_path = vec![0u16; 32_768];
    let final_len = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            final_path.as_mut_ptr(),
            final_path.len() as u32,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    let final_path = if final_len == 0 || final_len >= final_path.len() as u32 {
        unsafe {
            CloseHandle(handle);
        }
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "could not verify flow evidence root handle",
        )
        .into());
    } else {
        String::from_utf16_lossy(&final_path[..final_len as usize])
    };
    if !same_windows_path(&final_path, &name) {
        unsafe {
            CloseHandle(handle);
        }
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "flow evidence root handle escaped its canonical path",
        )
        .into());
    }

    // SAFETY: CreateFileW returned a valid owned directory handle and this
    // function transfers ownership exactly once to std::fs::File.
    // SAFETY: CreateFileW returned a valid owned directory handle and this
    // function transfers ownership exactly once to std::fs::File.
    Ok((
        unsafe { File::from_raw_handle(handle as _) },
        PathBuf::from(final_path),
    ))
}

#[cfg(windows)]
fn create_file_no_follow(parent: &File, filename: &OsStr) -> Result<File, io::Error> {
    nt_create_file(
        Some(parent),
        filename,
        windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_READ
            | windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_WRITE
            | windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE,
        windows_sys::Wdk::Storage::FileSystem::FILE_CREATE,
        windows_sys::Wdk::Storage::FileSystem::FILE_NON_DIRECTORY_FILE
            | windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_REPARSE_POINT
            | windows_sys::Wdk::Storage::FileSystem::FILE_SYNCHRONOUS_IO_NONALERT,
    )
}

#[cfg(windows)]
fn nt_create_file(
    parent: Option<&File>,
    name: &OsStr,
    desired_access: u32,
    disposition: u32,
    create_options: u32,
) -> Result<File, io::Error> {
    use std::mem::size_of;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use std::ptr::{null, null_mut};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::NtCreateFile;
    use windows_sys::Win32::Foundation::{
        HANDLE, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, UNICODE_STRING,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    let name = name
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "non-Unicode evidence path"))?;
    let mut wide = name.encode_utf16().collect::<Vec<_>>();
    if wide.len() > (u16::MAX as usize / 2) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "evidence path component is too long",
        ));
    }
    let mut unicode_name = UNICODE_STRING {
        Length: (wide.len() * 2) as u16,
        MaximumLength: (wide.len() * 2) as u16,
        Buffer: wide.as_mut_ptr(),
    };
    let mut attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent
            .map(|file| file.as_raw_handle() as HANDLE)
            .unwrap_or(null_mut()),
        ObjectName: &mut unicode_name,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        SecurityDescriptor: null(),
        SecurityQualityOfService: null_mut(),
    };
    let mut io_status = IO_STATUS_BLOCK::default();
    let mut handle: HANDLE = null_mut();
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            desired_access,
            &mut attributes,
            &mut io_status,
            null(),
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            disposition,
            create_options,
            null(),
            0,
        )
    };
    if status < 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("NtCreateFile failed with NTSTATUS 0x{:08x}", status as u32),
        ));
    }
    if handle.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "NtCreateFile returned a null handle",
        ));
    }

    // SAFETY: NtCreateFile returned a valid owned synchronous file handle and
    // ownership is transferred exactly once to std::fs::File.
    Ok(unsafe { File::from_raw_handle(handle as _) })
}

#[cfg(windows)]
fn win32_absolute_path(path: &Path) -> Result<String, io::Error> {
    let path = path
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "non-Unicode flow path"))?;
    if path.starts_with(r"\\?\") {
        return Ok(path.to_owned());
    }
    if let Some(unc) = path.strip_prefix(r"\\") {
        Ok(format!(r"\\?\UNC\{unc}"))
    } else {
        Ok(format!(r"\\?\{path}"))
    }
}

#[cfg(windows)]
fn same_windows_path(left: &str, right: &str) -> bool {
    lexically_normalize_windows_path(left)
        .eq_ignore_ascii_case(&lexically_normalize_windows_path(right))
}

#[cfg(windows)]
fn lexically_normalize_windows_path(path: &str) -> String {
    let path = path.replace('/', "\\");
    let (root, rest, minimum_components) = if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        (r"\\?\UNC\".to_owned(), rest, 2)
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        if rest.len() >= 3 && rest.as_bytes()[1] == b':' && rest.as_bytes()[2] == b'\\' {
            (format!(r"\\?\{}\", &rest[..2]), &rest[3..], 0)
        } else {
            (r"\\?\".to_owned(), rest, 0)
        }
    } else if let Some(rest) = path.strip_prefix(r"\\") {
        (r"\\".to_owned(), rest, 2)
    } else if path.len() >= 3 && path.as_bytes()[1] == b':' && path.as_bytes()[2] == b'\\' {
        (format!("{}\\", &path[..2]), &path[3..], 0)
    } else {
        (String::new(), path.as_str(), 0)
    };

    let mut components = Vec::new();
    for component in rest.split('\\') {
        match component {
            "" | "." => {}
            ".." => {
                if components.len() > minimum_components {
                    components.pop();
                }
            }
            component => components.push(component),
        }
    }

    let preserve_drive_root_separator = components.is_empty() && root.ends_with(":\\");
    let mut normalized = root;
    normalized.push_str(&components.join("\\"));
    if preserve_drive_root_separator {
        normalized
    } else {
        normalized.trim_end_matches('\\').to_owned()
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::lexically_normalize_windows_path;

    #[test]
    fn preserves_drive_root_separator_during_lexical_normalization() {
        assert_eq!(
            lexically_normalize_windows_path(r"\\?\C:\"),
            r"\\?\C:\"
        );
    }
}

#[cfg(test)]
mod collector_tests {
    use super::*;

    fn config(max_raw_bytes: usize, max_websocket_frames: usize) -> EvidenceConfig {
        EvidenceConfig {
            max_raw_bytes,
            max_websocket_frames,
            redact_fields: vec!["token".into(), "dev_key".into()],
        }
    }

    #[test]
    fn redacts_sensitive_fields_and_bounds_raw_payload() {
        let mut collector = EvidenceCollector::new(config(32, 2)).unwrap();
        let record = collector
            .capture_json(
                EvidenceKind::HttpResponse,
                &serde_json::json!({"token":"secret","nested":{"dev_key":"key"},"payload":"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"}),
            )
            .unwrap();
        assert!(record.redacted);
        assert!(record.truncated);
        assert!(record.payload.len() <= 32);
        assert!(!record.payload.contains("secret"));
    }

    #[test]
    fn websocket_frame_bound_is_explicit() {
        let mut collector = EvidenceCollector::new(config(100, 1)).unwrap();
        collector.capture_json(EvidenceKind::WebsocketFrame, &Value::Null).unwrap();
        assert_eq!(
            collector.capture_json(EvidenceKind::WebsocketFrame, &Value::Null),
            Err(EvidenceCaptureError::FrameLimitExceeded)
        );
    }
}
