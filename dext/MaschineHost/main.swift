//
// main.swift
//
// Minimal Maschine host app. On launch, submits an
// OSSystemExtensionRequest.activationRequest for the bundled dext and
// logs the delegate callbacks to stdout. No GUI — LSBackgroundOnly.
//

import Foundation
import SystemExtensions

let kDextBundleIdentifier = "com.cantonic.maschine.dext"

final class ActivationDelegate: NSObject, OSSystemExtensionRequestDelegate {
    func request(_ request: OSSystemExtensionRequest,
                 actionForReplacingExtension existing: OSSystemExtensionProperties,
                 withExtension ext: OSSystemExtensionProperties) -> OSSystemExtensionRequest.ReplacementAction {
        print("[MaschineHost] replacing existing dext \(existing.bundleVersion) -> \(ext.bundleVersion)")
        return .replace
    }

    func requestNeedsUserApproval(_ request: OSSystemExtensionRequest) {
        print("[MaschineHost] user approval needed — open System Settings → General → Login Items & Extensions")
    }

    func request(_ request: OSSystemExtensionRequest,
                 didFinishWithResult result: OSSystemExtensionRequest.Result) {
        print("[MaschineHost] activation finished: \(result.rawValue)")
        if result == .completed {
            exit(EXIT_SUCCESS)
        }
    }

    func request(_ request: OSSystemExtensionRequest,
                 didFailWithError error: Error) {
        print("[MaschineHost] activation failed: \(error)")
        exit(EXIT_FAILURE)
    }
}

let delegate = ActivationDelegate()
let request = OSSystemExtensionRequest.activationRequest(
    forExtensionWithIdentifier: kDextBundleIdentifier,
    queue: .main
)
request.delegate = delegate
OSSystemExtensionManager.shared.submitRequest(request)
print("[MaschineHost] submitted activation request for \(kDextBundleIdentifier)")

RunLoop.main.run()
