//
//  SasayakiSource.swift
//  Hoshi Reader
//
//  Copyright © 2026 Manhhao.
//  SPDX-License-Identifier: GPL-3.0-or-later
//

import Foundation

nonisolated struct SasayakiSource: Sendable {
    struct Chapter: Sendable {
        let chapterIndex: Int
        let start: Int
        let length: Int
        var end: Int { start + length }
    }
    
    let text: [Character]
    let sentenceEnds: [Bool]
    let segmentEnds: [Bool]
    let chapters: [Chapter]
    let images: [SasayakiImage]
    
    static let characterClass = "0-9A-Za-z○◯々-〇〻ぁ-ゖゝ-ゞァ-ヺー０-９Ａ-Ｚａ-ｚｦ-ﾝ가-힣ㄱ-ㆎ\\p{Radical}\\p{Unified_Ideograph}"
    static let ttuRegex = try! NSRegularExpression(pattern: "[" + characterClass + "]")
    
    static func findText(source: [Character], text: [Character], start: Int, end: Int) -> Int? {
        var index = start
        while index <= end - text.count {
            if source[index..<(index + text.count)].elementsEqual(text) {
                return index
            }
            index += 1
        }
        return nil
    }
}
