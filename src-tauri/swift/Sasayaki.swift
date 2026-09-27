//
//  Sasayaki.swift
//  Hoshi Reader
//
//  Copyright © 2026 Manhhao.
//  SPDX-License-Identifier: GPL-3.0-or-later
//

import Foundation

struct SasayakiToken: Codable, Sendable {
    let text: String
    let start: Double
    let end: Double
}

struct SasayakiMatch: Codable, Identifiable, Hashable, Sendable {
    let id: String
    let startTime: Double
    var endTime: Double
    let text: String
    let chapterIndex: Int
    let start: Int
    let length: Int
}

struct SasayakiImage: Codable, Sendable {
    let chapterIndex: Int
    let imageIndex: Int
    let offset: Int
}

nonisolated struct SasayakiMatchData: Codable, Sendable {
    let matches: [SasayakiMatch]
    let unmatched: Int
    let images: [SasayakiImage]
    
    var matchedCharacters: Int {
        matches.reduce(0) { $0 + $1.length }
    }
}
