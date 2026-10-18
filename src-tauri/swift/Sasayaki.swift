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
