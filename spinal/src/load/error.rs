use core::fmt;

/// The input document associated with a loader error.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LoadDocument {
    /// Spine skeleton JSON.
    SkeletonJson,
    /// Spine text-atlas data.
    Atlas,
}

/// A stable machine-readable loader failure category.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LoadErrorKind {
    /// Input bytes were not valid UTF-8.
    InvalidUtf8,
    /// The document was not syntactically valid.
    Syntax,
    /// A value had the wrong shape or a required field was missing.
    SchemaViolation,
    /// Version metadata was missing or malformed.
    InvalidVersion,
    /// The skeleton uses a wire-format version outside the target profile.
    UnsupportedVersion,
    /// A required numeric value was not finite or representable.
    NonFiniteNumber,
    /// A JSON object field was repeated.
    DuplicateField,
    /// A required identity was duplicated.
    DuplicateName,
    /// Authored evaluation or key order was invalid.
    InvalidOrder,
    /// The bone or constraint graph was invalid.
    InvalidTopology,
    /// A required authored reference could not be resolved.
    UnresolvedReference,
    /// A rigid region attachment had no matching atlas region.
    MissingAtlasRegion,
    /// A rigid region attachment matched multiple atlas regions.
    AmbiguousAtlasRegion,
    /// Unsupported data could not be skipped at a safe record boundary.
    UnsupportedData,
    /// A validated input exceeded an indexable runtime table.
    CapacityExceeded,
}

/// A best-available location in one input document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceLocation {
    document: LoadDocument,
    path: Option<Box<str>>,
    line: Option<usize>,
    column: Option<usize>,
    byte_offset: Option<usize>,
}

impl SourceLocation {
    pub(crate) fn for_document(document: LoadDocument) -> Self {
        Self {
            document,
            path: None,
            line: None,
            column: None,
            byte_offset: None,
        }
    }

    pub(crate) fn with_path(mut self, path: impl Into<Box<str>>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub(crate) const fn with_text_position(
        mut self,
        line: usize,
        column: usize,
        byte_offset: Option<usize>,
    ) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self.byte_offset = byte_offset;
        self
    }

    /// Returns the document containing this location.
    #[must_use]
    pub const fn document(&self) -> LoadDocument {
        self.document
    }

    /// Returns an RFC 6901 structural pointer when one is available.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Returns the one-based line when known.
    #[must_use]
    pub const fn line(&self) -> Option<usize> {
        self.line
    }

    /// Returns the one-based column when known.
    #[must_use]
    pub const fn column(&self) -> Option<usize> {
        self.column
    }

    /// Returns the zero-based byte offset when known.
    #[must_use]
    pub const fn byte_offset(&self) -> Option<usize> {
        self.byte_offset
    }
}

/// An owned loader error independent of the private parsing libraries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadError {
    kind: LoadErrorKind,
    message: Box<str>,
    location: SourceLocation,
    related_locations: Box<[SourceLocation]>,
}

impl LoadError {
    pub(crate) fn new(
        kind: LoadErrorKind,
        message: impl Into<Box<str>>,
        location: SourceLocation,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            location,
            related_locations: Box::default(),
        }
    }

    pub(crate) fn with_related_locations(
        mut self,
        related_locations: impl Into<Box<[SourceLocation]>>,
    ) -> Self {
        self.related_locations = related_locations.into();
        self
    }

    /// Returns the stable failure category.
    #[must_use]
    pub const fn kind(&self) -> LoadErrorKind {
        self.kind
    }

    /// Returns a human-readable explanation.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the primary best-available source location.
    #[must_use]
    pub const fn location(&self) -> &SourceLocation {
        &self.location
    }

    /// Returns the structural pointer from the primary location.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        self.location.path()
    }

    /// Returns other source locations relevant to the failure.
    #[must_use]
    pub fn related_locations(&self) -> &[SourceLocation] {
        &self.related_locations
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)?;
        if let Some(path) = self.location.path() {
            write!(formatter, " at {path}")?;
        }
        if let Some(line) = self.location.line() {
            write!(formatter, " (line {line}")?;
            if let Some(column) = self.location.column() {
                write!(formatter, ", column {column}")?;
            }
            write!(formatter, ")")?;
        }
        Ok(())
    }
}

impl std::error::Error for LoadError {}
