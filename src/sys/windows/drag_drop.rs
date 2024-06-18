use std::{
    cell::Cell,
    ffi::{c_void, OsStr},
    os::windows::ffi::OsStrExt,
    sync::Once,
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::Com::*,
        System::Memory::*,
        System::Ole::{DoDragDrop, IDropSource, IDropSource_Impl, CF_HDROP, DROPEFFECT, DROPEFFECT_COPY},
        System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS},
        UI::Shell::DROPFILES,
    },
};

static OLE_UNINITIALIZE: Once = Once::new();
fn init_ole() {
    let _ = OLE_UNINITIALIZE.call_once(|| {
        use windows::Win32::System::Ole::{OleInitialize};
        let _ = unsafe { OleInitialize(std::ptr::null_mut()) }.unwrap();

        // I guess we never deinitialize for now?
        // OleUninitialize
    });
}

const SUPPORTED_FORMATS: [FORMATETC; 1] = [
    FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: 0,
        tymed: TYMED_HGLOBAL.0 as u32,
    },
];

#[implement(IDataObject)]
struct DataObject(isize);

#[implement(IDropSource)]
struct DropSource(());

#[implement(IEnumFORMATETC)]
struct FormatEnumerator {
    formats: &'static [FORMATETC],
    current: Cell<usize>,
}

impl FormatEnumerator {
    pub fn new(formats: &'static [FORMATETC]) -> Self {
        return Self { formats, current: Cell::new(0) };
    }
}

#[allow(non_snake_case)]
impl IEnumFORMATETC_Impl for FormatEnumerator {
    fn Next(&self, celt: u32, rgelt: *mut FORMATETC, pceltFetched: *mut u32) -> Result<()> {
        if celt != 1 && pceltFetched.is_null() {
            return Err(Error::new(S_FALSE, HSTRING::new()));
        }

        let current = self.current.get();
        let count = std::cmp::min(celt as usize, self.formats.len() - current);
        if count == 0 {
            return Err(Error::new(S_FALSE, HSTRING::new()));
        }

        let output: &mut [FORMATETC] = unsafe { std::slice::from_raw_parts_mut(rgelt, celt as usize) };
        for (idx, fmt) in (&self.formats[current..count]).iter().enumerate() {
            output[idx] = fmt.clone();
        }

        if !pceltFetched.is_null() {
            unsafe { std::ptr::write(pceltFetched, count as u32) };
        }

        self.current.set(current + count);
        return Ok(());
    }

    fn Skip(&self, celt: u32) -> Result<()> {
        let current = std::cmp::min(self.current.get() + (celt as usize), self.formats.len());
        self.current.set(current);
        return Ok(());
    }

    fn Reset(&self) -> Result<()> {
        self.current.set(0);
        return Ok(());
    }

    fn Clone(&self) -> Result<IEnumFORMATETC> {
        return Ok(FormatEnumerator {
            formats: self.formats,
            current: self.current.clone(),
        }.into());
    }
}

impl DropSource {
    fn new() -> Self {
        return Self(());
    }
}

impl DataObject {
    fn new(handle: isize) -> Self {
        return Self(handle);
    }

    fn is_supported_format(pformatetc: *const FORMATETC) -> bool {
        if let Some(format_etc) = unsafe { pformatetc.as_ref() } {
            if format_etc.tymed as i32 != TYMED_HGLOBAL.0 {
                return false;
            }
            if format_etc.cfFormat != CF_HDROP.0 {
                return false;
            }
            if format_etc.dwAspect != DVASPECT_CONTENT.0 {
                return false;
            }
            return true;
        } else {
            return false;
        }
    }
}

#[allow(non_snake_case)]
impl IDataObject_Impl for DataObject {
    fn GetData(&self, pformatetc: *const FORMATETC) -> Result<STGMEDIUM> {
        if let Some(fmt) = unsafe { pformatetc.as_ref() } {
            if fmt.tymed != TYMED_HGLOBAL.0 as u32 {
                return Err(Error::new(STG_E_MEDIUMFULL, HSTRING::new()));
            }
        }

        if Self::is_supported_format(pformatetc) {
            return Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL,
                Anonymous: STGMEDIUM_0 { hGlobal: self.0 },
                pUnkForRelease: None.into(),
            });
        } else {
            return Err(Error::new(S_FALSE, HSTRING::new()));
        }
    }

    fn GetDataHere(&self, _pformatetc: *const FORMATETC, _pmedium: *mut STGMEDIUM) -> Result<()> {
        return Err(Error::new(DV_E_FORMATETC, HSTRING::new()));
    }

    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        if let Some(fmt) = unsafe { pformatetc.as_ref() } {
            if fmt.dwAspect != DVASPECT_CONTENT.0 {
                return DV_E_DVASPECT;
            }

            if fmt.cfFormat != CF_HDROP.0 {
                return DV_E_FORMATETC;
            }

            // @Remark:
            // Somehow if we do this check Visual Studio doesn't query for other "tymed",
            // so after failing for "TYMED_STREAM", it stop the process of finding out
            // supported format.
            //
            if fmt.tymed != TYMED_HGLOBAL.0 as u32 {
                return DV_E_TYMED;
            }

            return S_OK;
        } else {
            return E_INVALIDARG;
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        _pformatectin: *const FORMATETC,
        pformatetcout: *mut FORMATETC,
    ) -> HRESULT {
        unsafe { (*pformatetcout).ptd = std::ptr::null_mut() };
        return E_NOTIMPL;
    }

    fn SetData(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *const STGMEDIUM,
        _frelease: BOOL,
    ) -> Result<()> {
        return Err(Error::new(E_NOTIMPL, HSTRING::new()));
    }

    fn EnumFormatEtc(&self, _dwdirection: u32) -> Result<IEnumFORMATETC> {
        // @Remark:
        // We need to support format enumeration for Visual Studio even though
        // it completely ignore what we return...
        return Ok(FormatEnumerator::new(&SUPPORTED_FORMATS).into())
    }

    fn DAdvise(
        &self,
        _pformatetc: *const FORMATETC,
        _advf: u32,
        _padvsink: &Option<IAdviseSink>,
    ) -> Result<u32> {
        return Err(Error::new(OLE_E_ADVISENOTSUPPORTED, HSTRING::new()));
    }

    fn DUnadvise(&self, _dwconnection: u32) -> Result<()> {
        return Err(Error::new(OLE_E_ADVISENOTSUPPORTED, HSTRING::new()));
    }

    fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
        return Err(Error::new(OLE_E_ADVISENOTSUPPORTED, HSTRING::new()));
    }
}

impl IDropSource_Impl for DropSource {
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        if fescapepressed.as_bool() {
            return DRAGDROP_S_CANCEL;
        }
        if (grfkeystate & MK_LBUTTON) == MODIFIERKEYS_FLAGS(0) {
            return DRAGDROP_S_DROP;
        }
        return S_OK;
    }

    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        return DRAGDROP_S_USEDEFAULTCURSORS;
    }
}

pub fn enter_drag_drop(paths: &[&str]) {
    init_ole();

    let mut buffer = Vec::new();
    for path in paths {
        let path = OsStr::new(path);
        for code in path.encode_wide() {
            buffer.push(code);
        }
        buffer.push(0);
    }

    // We finish with a double null.
    buffer.push(0);

    let size = std::mem::size_of::<DROPFILES>() + buffer.len() * 2;
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, size) };
    let ptr = unsafe { GlobalLock(handle) };

    let header = ptr as *mut DROPFILES;
    unsafe {
        (*header).pFiles = std::mem::size_of::<DROPFILES>() as u32;
        (*header).fWide = BOOL(1);
    }

    unsafe {
        std::ptr::copy(
            buffer.as_ptr() as *const c_void,
            ptr.add(std::mem::size_of::<DROPFILES>()),
            buffer.len() * 2,
        )
    };
    unsafe { GlobalUnlock(handle) };

    let data_object = DataObject::new(handle).into();
    let drop_source = DropSource::new().into();

    let mut effect = DROPEFFECT(0);
    let _ = unsafe { DoDragDrop(&data_object, &drop_source, DROPEFFECT_COPY, &mut effect) };
}
