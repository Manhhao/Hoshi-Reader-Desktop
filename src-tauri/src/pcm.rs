use std::path::Path;

pub type Pcm = (Vec<i16>, u32, usize);

#[cfg(windows)]
pub fn decode(path: &Path, start: f64, end: f64, mime: &str) -> Option<Pcm> {
    use windows::Win32::Media::MediaFoundation::{
        MF_VERSION, MFSTARTUP_LITE, MFShutdown, MFStartup,
    };
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

    unsafe {
        let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let pcm = MFStartup(MF_VERSION, MFSTARTUP_LITE).ok().and_then(|_| {
            let pcm = read_media_foundation(path, start, end, mime).ok();
            let _ = MFShutdown();
            pcm
        });
        if com {
            CoUninitialize();
        }
        pcm
    }
}

#[cfg(windows)]
unsafe fn read_media_foundation(
    path: &Path,
    start: f64,
    end: f64,
    mime: &str,
) -> windows::core::Result<Pcm> {
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::core::{GUID, HSTRING, Interface};

    unsafe {
        let stream = MFCreateFile(
            MF_ACCESSMODE_READ,
            MF_OPENMODE_FAIL_IF_NOT_EXIST,
            MF_FILEFLAGS_NONE,
            &HSTRING::from(path.as_os_str()),
        )?;
        stream
            .cast::<IMFAttributes>()?
            .SetString(&MF_BYTESTREAM_CONTENT_TYPE, &HSTRING::from(mime))?;
        let reader = MFCreateSourceReaderFromByteStream(&stream, None)?;
        let audio = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader.SetStreamSelection(audio, true)?;

        let output = MFCreateMediaType()?;
        output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        output.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
        output.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
        reader.SetCurrentMediaType(audio, None, &output)?;
        let format = reader.GetCurrentMediaType(audio)?;
        let rate = format.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND)?;
        let channels = format.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS)? as usize;

        reader.SetCurrentPosition(&GUID::zeroed(), &PROPVARIANT::from((start * 1e7) as i64))?;

        let mut samples = Vec::new();
        loop {
            let mut flags = 0;
            let mut timestamp = 0;
            let mut sample = None;
            reader.ReadSample(
                audio,
                0,
                None,
                Some(&mut flags),
                Some(&mut timestamp),
                Some(&mut sample),
            )?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
            let Some(sample) = sample else {
                continue;
            };
            let time = timestamp as f64 / 1e7;
            if time > end {
                break;
            }
            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut data = std::ptr::null_mut();
            let mut len = 0;
            buffer.Lock(&mut data, None, Some(&mut len))?;
            let chunk = std::slice::from_raw_parts(data as *const i16, len as usize / 2);
            let skip = ((start - time).max(0.0) * rate as f64) as usize * channels;
            samples.extend_from_slice(&chunk[skip.min(chunk.len())..]);
            buffer.Unlock()?;
        }
        samples.truncate(((end - start) * rate as f64) as usize * channels);
        Ok((samples, rate, channels))
    }
}

#[cfg(target_os = "macos")]
pub fn decode(path: &Path, start: f64, end: f64, _mime: &str) -> Option<Pcm> {
    use std::ffi::{CString, c_char, c_void};
    use std::os::unix::ffi::OsStrExt;

    unsafe extern "C" {
        fn hoshi_decode_audio(
            path: *const c_char,
            start: f64,
            end: f64,
            context: *mut c_void,
            callback: extern "C" fn(*mut c_void, *const i16, usize, u32, u32),
        );
    }

    extern "C" fn receive(
        context: *mut c_void,
        data: *const i16,
        count: usize,
        rate: u32,
        channels: u32,
    ) {
        let pcm = unsafe { &mut *(context as *mut Option<Pcm>) };
        *pcm = Some((
            unsafe { std::slice::from_raw_parts(data, count) }.to_vec(),
            rate,
            channels as usize,
        ));
    }

    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut pcm: Option<Pcm> = None;
    unsafe {
        hoshi_decode_audio(
            path.as_ptr(),
            start,
            end,
            &mut pcm as *mut Option<Pcm> as *mut c_void,
            receive,
        )
    };
    pcm
}
