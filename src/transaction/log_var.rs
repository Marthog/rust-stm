use std::any::Any;
use std::sync::Arc;

pub type ArcAny = Arc<Any + Send + Sync>;

/// LogVar is used by `Log` to track which `Var` was either read or written
#[derive(Clone)]
pub enum LogVar {
    /// Var has been read.
    Read(ArcAny),
    
    /// Var has been written and no dependency on the original exists.
    ///
    /// There is no need to check for consistency.
    Write(ArcAny),

    /// ReadWrite(original value, temporary stored value)
    ///
    /// Var has been read first and then written.
    ///
    /// It needs to be checked for consistency.
    ReadWrite(ArcAny, ArcAny),

    /// Var has been read on blocked path.
    ///
    /// Don't check for consistency, but block on Var,
    /// so that the threat wakes up when the first path
    /// has been unlocked.
    ReadObsolete(ArcAny),

    /// ReadWriteObsolete(original value, temporary stored value)
    ///
    /// Var has been read on blocked path and then written to.
    ///
    /// Don't check for consistency, but block on Var,
    /// so that the threat wakes up when the first path
    /// has been unlocked.
    ReadObsoleteWrite(ArcAny, ArcAny)
}


impl LogVar {
    /// read a value and potentially upgrade the state
    pub fn read(&mut self) -> ArcAny {
        use self::LogVar::*;

        let this = self.clone();

        match this {
            // Use last read value or get written one
            Read(v) | Write(v) | ReadWrite(_,v) | ReadObsoleteWrite(_,v)     => v,

            // Upgrade to a real Read
            ReadObsolete(v)           => {
                *self = Read(v.clone());
                v
            }
        }
    }
    
    /// write a value and potentially upgrade the state.
    pub fn write(&mut self, w: ArcAny)
    {
        use self::LogVar::*;

        let this = self.clone();

        *self = match this {
            Write(_)    
                => Write(w),

            // Register write
            ReadObsolete(r) | ReadObsoleteWrite(r, _)
                => ReadObsoleteWrite(r, w),

            // Register write
            Read(r) | ReadWrite(r, _)
                => ReadWrite(r, w),
        };
    }

    /// turn this into an obsolete version
    pub fn obsolete(self) -> Option<LogVar>
    {
        use self::LogVar::*;
        self.into_read_value()
            .map(|a| ReadObsolete(a))
    }

    /// Ignore all Write... and get the original value of a Var.
    pub fn into_read_value(self) -> Option<ArcAny> {
        use self::LogVar::*;
        match self {
            Read(v) | ReadWrite(v,_) | ReadObsolete(v) | ReadObsoleteWrite(v,_)
                => Some(v),
            Write(_)    => None,
        }
    }
}

/// Test if writes are ignored, when a var is set to obsolete
#[test]
fn test_write_obsolete_ignore() {
    let t = LogVar::Write(Arc::new(42)).obsolete();
    assert!(t.is_none());
}

