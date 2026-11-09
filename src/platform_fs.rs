//! Private local filesystem boundary. Unix keeps owner/mode/O_NOFOLLOW checks;
//! Windows uses protected current-user/SYSTEM DACLs and handle-based validation.
//! No credential contents are returned in errors.
use std::{
    fs::{File, OpenOptions},
    io,
    path::{Path, PathBuf},
};
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let value = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
    #[cfg(not(windows))]
    let value = std::env::var_os("HOME");
    value.map(PathBuf::from)
}
pub fn home() -> Option<PathBuf> {
    home_dir()
}

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Expected private regular filesystem object owned by current user",
    )
}
pub fn read(path: &Path, private: bool) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.read(true);
    imp::configure(&mut opts);
    let file = opts.open(path)?;
    imp::validate(&file, false, private, false)?;
    Ok(file)
}
pub fn create_new(path: &Path) -> io::Result<File> {
    imp::write(path, false)
}
pub fn append(path: &Path) -> io::Result<File> {
    imp::write(path, true)
}
pub fn directory(path: &Path, allow_existing: bool) -> io::Result<()> {
    match imp::mkdir(path) {
        Ok(()) => (),
        Err(e) if allow_existing && e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e),
    }
    check(path, true)
}
pub fn check(path: &Path, dir: bool) -> io::Result<()> {
    let mut opts = OpenOptions::new();
    opts.read(true);
    imp::configure(&mut opts);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        opts.access_mode(windows_sys::Win32::Storage::FileSystem::READ_CONTROL);
    }
    let file = opts.open(path)?;
    imp::validate(&file, dir, true, false)
}
#[cfg(unix)]
mod imp {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    pub fn configure(o: &mut OpenOptions) {
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    pub fn validate(f: &File, dir: bool, private: bool, single: bool) -> io::Result<()> {
        let m = f.metadata()?;
        if (if dir { !m.is_dir() } else { !m.is_file() })
            || (private && (m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0))
            || (single && m.nlink() != 1)
        {
            return Err(denied());
        }
        Ok(())
    }
    pub fn mkdir(p: &Path) -> io::Result<()> {
        std::fs::DirBuilder::new().mode(0o700).create(p)
    }
    pub fn write(p: &Path, append: bool) -> io::Result<File> {
        let mut o = OpenOptions::new();
        o.write(true).mode(0o600);
        configure(&mut o);
        if append {
            o.append(true).create(true);
        } else {
            o.create_new(true);
        }
        let f = o.open(p)?;
        validate(&f, false, true, true)?;
        Ok(f)
    }
}
#[cfg(windows)]
mod imp {
    use super::*;
    use std::{
        os::windows::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
            io::{AsRawHandle, FromRawHandle},
        },
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE, LocalFree},
        Security::Authorization::*,
        Security::*,
        Storage::FileSystem::*,
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    struct Local(*mut std::ffi::c_void);
    impl Drop for Local {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    fn current_sid() -> io::Result<Vec<u8>> {
        unsafe {
            let mut token = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut needed = 0;
            GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut needed);
            // usize backing guarantees TOKEN_USER alignment.
            let mut data = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
            let ok = GetTokenInformation(
                token,
                TokenUser,
                data.as_mut_ptr().cast(),
                needed,
                &mut needed,
            );
            CloseHandle(token);
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            let sid = (*(data.as_ptr().cast::<TOKEN_USER>())).User.Sid;
            let n = GetLengthSid(sid);
            let mut copy = vec![0u8; n as usize];
            if CopySid(n, copy.as_mut_ptr().cast(), sid) == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(copy)
        }
    }
    fn security() -> io::Result<Local> {
        unsafe {
            let sid = current_sid()?;
            let mut str_sid = ptr::null_mut();
            if ConvertSidToStringSidW(sid.as_ptr() as _, &mut str_sid) == 0 {
                return Err(io::Error::last_os_error());
            }
            let sid_guard = Local(str_sid.cast());
            let mut n = 0;
            while *str_sid.add(n) != 0 {
                n += 1;
            }
            let sid_text =
                String::from_utf16(std::slice::from_raw_parts(str_sid, n)).map_err(|_| denied())?;
            let sddl: Vec<u16> = format!("O:{sid_text}D:P(A;;FA;;;SY)(A;;FA;;;{sid_text})")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let mut sd = ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut sd,
                ptr::null_mut(),
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            drop(sid_guard);
            Ok(Local(sd))
        }
    }
    pub fn configure(o: &mut OpenOptions) {
        o.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    }
    pub fn validate(f: &File, dir: bool, private: bool, single: bool) -> io::Result<()> {
        let m = f.metadata()?;
        if m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || (if dir { !m.is_dir() } else { !m.is_file() })
        {
            return Err(denied());
        }
        unsafe {
            if single {
                let mut info = std::mem::zeroed();
                if GetFileInformationByHandle(f.as_raw_handle(), &mut info) == 0 {
                    return Err(io::Error::last_os_error());
                }
                if info.nNumberOfLinks != 1 {
                    return Err(denied());
                }
            }
            if !private {
                return Ok(());
            }
            let sid = current_sid()?;
            let mut owner = ptr::null_mut();
            let mut acl = ptr::null_mut();
            let mut sd = ptr::null_mut();
            let error = GetSecurityInfo(
                f.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                &mut acl,
                ptr::null_mut(),
                &mut sd,
            );
            if error != 0 {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
            let _guard = Local(sd);
            if owner.is_null() || EqualSid(owner, sid.as_ptr() as _) == 0 || acl.is_null() {
                return Err(denied());
            }
            for index in 0..(*acl).AceCount as u32 {
                let mut ace = ptr::null_mut();
                if GetAce(acl, index, &mut ace) == 0 {
                    return Err(io::Error::last_os_error());
                }
                let head = &*(ace.cast::<ACE_HEADER>());
                // Ignore deny ACEs; reject unrecognized grants (object/callback forms too).
                if head.AceType == 1
                /* ACCESS_DENIED_ACE_TYPE */
                {
                    continue;
                }
                if head.AceType != 0
                /* ACCESS_ALLOWED_ACE_TYPE */
                {
                    return Err(denied());
                }
                let grant = &*(ace.cast::<ACCESS_ALLOWED_ACE>());
                let trustee = ptr::addr_of!(grant.SidStart) as PSID;
                if EqualSid(trustee, sid.as_ptr() as _) == 0
                    && IsWellKnownSid(trustee, WinLocalSystemSid) == 0
                    && IsWellKnownSid(trustee, WinBuiltinAdministratorsSid) == 0
                {
                    return Err(denied());
                }
            }
        }
        Ok(())
    }
    pub fn mkdir(p: &Path) -> io::Result<()> {
        let sd = security()?;
        let attrs = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        let path: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe { CreateDirectoryW(path.as_ptr(), &attrs) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    pub fn write(p: &Path, append: bool) -> io::Result<File> {
        let sd = security()?;
        let mut attrs = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        let path: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
        let access = if append {
            FILE_APPEND_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE
        } else {
            FILE_GENERIC_WRITE | READ_CONTROL
        };
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                &mut attrs,
                if append { OPEN_ALWAYS } else { CREATE_NEW },
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_ATTRIBUTE_NORMAL,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let f = unsafe { File::from_raw_handle(handle) };
        validate(&f, false, true, true)?;
        Ok(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    fn temp() -> PathBuf {
        std::env::temp_dir().join(format!("dot-platform-test-{}", uuid::Uuid::new_v4()))
    }
    #[test]
    fn private_create_read_append_and_directory() {
        let dir = temp();
        directory(&dir, false).unwrap();
        check(&dir, true).unwrap();
        let path = dir.join("state");
        create_new(&path).unwrap().write_all(b"first").unwrap();
        assert!(create_new(&path).is_err());
        append(&path).unwrap().write_all(b"second").unwrap();
        let mut value = String::new();
        read(&path, true)
            .unwrap()
            .read_to_string(&mut value)
            .unwrap();
        assert_eq!(value, "firstsecond");
        assert!(read(&dir, true).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_broad_modes_and_append_hardlinks() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = temp();
        directory(&dir, false).unwrap();
        let path = dir.join("state");
        drop(create_new(&path).unwrap());
        symlink(&path, dir.join("link")).unwrap();
        assert!(read(&dir.join("link"), false).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read(&path, true).is_err());
        assert!(read(&path, false).is_ok());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::hard_link(&path, dir.join("hard")).unwrap();
        assert!(append(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn rejects_everyone_acl_for_private_reads() {
        use std::{os::windows::ffi::OsStrExt, ptr};
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{
                Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
                DACL_SECURITY_INFORMATION, SetFileSecurityW,
            },
        };
        let dir = temp();
        directory(&dir, false).unwrap();
        let path = dir.join("state");
        drop(create_new(&path).unwrap());
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let sddl: Vec<u16> = "D:P(A;;FA;;;WD)".encode_utf16().chain(Some(0)).collect();
        unsafe {
            let mut sd = ptr::null_mut();
            assert_ne!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut sd,
                    ptr::null_mut()
                ),
                0
            );
            let result = SetFileSecurityW(wide.as_ptr(), DACL_SECURITY_INFORMATION, sd);
            LocalFree(sd);
            assert_ne!(result, 0);
        }
        assert!(read(&path, true).is_err());
        assert!(read(&path, false).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
