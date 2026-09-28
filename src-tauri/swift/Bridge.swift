import AVFoundation
import Foundation
import Speech

struct AlignmentSource: Decodable {
    struct Chapter: Decodable {
        let chapterIndex: Int
        let start: Int
        let length: Int
    }

    let text: [String]
    let sentenceEnds: [Bool]
    let segmentEnds: [Bool]
    let chapters: [Chapter]
    let images: [SasayakiImage]
}

final class Transcription {
    var task: Task<Void, Never>?
    var finished = false
}

@_cdecl("hoshi_transcriber_status")
func transcriberStatus(context: UnsafeMutableRawPointer, callback: @convention(c) (UnsafeMutableRawPointer, Int32) -> Void) {
    let context = Int(bitPattern: context)
    Task {
        let reply = { (status: Int32) in callback(UnsafeMutableRawPointer(bitPattern: context)!, status) }
        guard #available(macOS 26.0, *) else {
            return reply(1)
        }
        guard SpeechTranscriber.isAvailable else {
            return reply(2)
        }
        let japanese = await SpeechTranscriber.supportedLocales.contains {
            $0.identifier(.bcp47).lowercased().hasPrefix("ja")
        }
        reply(japanese ? 0 : 3)
    }
}

@_cdecl("hoshi_audio_duration")
func audioDuration(path: UnsafePointer<CChar>) -> Double {
    guard let audio = try? AVAudioFile(forReading: URL(fileURLWithPath: String(cString: path))) else {
        return -1
    }
    return Double(audio.length) / audio.processingFormat.sampleRate
}

@_cdecl("hoshi_transcribe")
func transcribe(
    path: UnsafePointer<CChar>,
    from startTime: Double,
    context: UnsafeMutableRawPointer,
    onDownload: @convention(c) (UnsafeMutableRawPointer, Double) -> Void,
    onTokens: @convention(c) (UnsafeMutableRawPointer, UnsafePointer<CChar>, Double) -> Void,
    onFinish: @convention(c) (UnsafeMutableRawPointer, UnsafePointer<CChar>?) -> Void
) -> UnsafeMutableRawPointer {
    let url = URL(fileURLWithPath: String(cString: path))
    let context = Int(bitPattern: context)
    let pointer = { UnsafeMutableRawPointer(bitPattern: context)! }
    let transcription = Transcription()
    transcription.task = Task {
        let activity = ProcessInfo.processInfo.beginActivity(
            options: [.userInitiated, .idleSystemSleepDisabled],
            reason: "Transcribing audiobook"
        )
        defer { ProcessInfo.processInfo.endActivity(activity) }
        let message: String?
        if #available(macOS 26.0, *) {
            do {
                let file = try AVAudioFile(forReading: url)
                try await SasayakiTranscriber.transcribe(file: file, from: startTime) { fraction in
                    if !transcription.finished {
                        onDownload(pointer(), fraction)
                    }
                } onTokens: { tokens, through in
                    let json = String(decoding: try! JSONEncoder().encode(tokens), as: UTF8.self)
                    json.withCString { onTokens(pointer(), $0, through) }
                }
                message = nil
            } catch is CancellationError {
                message = nil
            } catch {
                message = error.localizedDescription
            }
        } else {
            message = "Transcription requires macOS 26."
        }
        await MainActor.run {
            transcription.finished = true
            if let message {
                message.withCString { onFinish(pointer(), $0) }
            } else {
                onFinish(pointer(), nil)
            }
        }
    }
    return Unmanaged.passRetained(transcription).toOpaque()
}

@_cdecl("hoshi_transcribe_cancel")
func cancelTranscription(handle: UnsafeMutableRawPointer) {
    Unmanaged<Transcription>.fromOpaque(handle).takeUnretainedValue().task?.cancel()
}

@_cdecl("hoshi_transcribe_release")
func releaseTranscription(handle: UnsafeMutableRawPointer) {
    Unmanaged<Transcription>.fromOpaque(handle).release()
}

@_cdecl("hoshi_align")
func align(
    source: UnsafePointer<CChar>,
    tokens: UnsafePointer<CChar>,
    context: UnsafeMutableRawPointer,
    callback: @convention(c) (UnsafeMutableRawPointer, UnsafePointer<CChar>) -> Void
) {
    autoreleasepool {
        let decoder = JSONDecoder()
        let input = try! decoder.decode(AlignmentSource.self, from: Data(String(cString: source).utf8))
        let tokens = try! decoder.decode([SasayakiToken].self, from: Data(String(cString: tokens).utf8))
        let source = SasayakiSource(
            text: input.text.map(Character.init),
            sentenceEnds: input.sentenceEnds,
            segmentEnds: input.segmentEnds,
            chapters: input.chapters.map {
                SasayakiSource.Chapter(chapterIndex: $0.chapterIndex, start: $0.start, length: $0.length)
            },
            images: input.images
        )
        let result = SasayakiAligner.align(source: source, tokens: tokens)
        String(decoding: try! JSONEncoder().encode(result), as: UTF8.self).withCString { callback(context, $0) }
    }
}

@_cdecl("hoshi_decode_audio")
func decodeAudio(
    path: UnsafePointer<CChar>,
    from start: Double,
    to end: Double,
    context: UnsafeMutableRawPointer,
    callback: @convention(c) (UnsafeMutableRawPointer, UnsafePointer<Int16>, Int, UInt32, UInt32) -> Void
) {
    let url = URL(fileURLWithPath: String(cString: path))
    guard let file = try? AVAudioFile(forReading: url, commonFormat: .pcmFormatInt16, interleaved: true) else {
        return
    }
    let format = file.processingFormat
    let first = min(AVAudioFramePosition(start * format.sampleRate), file.length)
    let last = min(AVAudioFramePosition(end * format.sampleRate), file.length)
    guard last > first,
          let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(last - first))
    else {
        return
    }
    file.framePosition = first
    guard (try? file.read(into: buffer)) != nil, let data = buffer.int16ChannelData else {
        return
    }
    let channels = Int(format.channelCount)
    callback(context, data[0], Int(buffer.frameLength) * channels, UInt32(format.sampleRate), UInt32(channels))
}
