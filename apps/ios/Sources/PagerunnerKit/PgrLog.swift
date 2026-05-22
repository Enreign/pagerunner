import Foundation
import os

/// Namespaced `os.Logger` instances for the app.
///
/// To follow the stream while a device is plugged into the Mac:
///
///     log stream --style compact \
///       --predicate 'subsystem == "com.enreign.pagerunner.ios"'
///
/// Or in Xcode: Window → Devices and Simulators → your phone → Open Console.
public enum PgrLog {
    private static let subsystem = "com.enreign.pagerunner.ios"

    public static let app        = Logger(subsystem: subsystem, category: "app")
    public static let connection = Logger(subsystem: subsystem, category: "connection")
    public static let websocket  = Logger(subsystem: subsystem, category: "websocket")
    public static let api        = Logger(subsystem: subsystem, category: "api")
    public static let chat       = Logger(subsystem: subsystem, category: "chat")
    public static let agent      = Logger(subsystem: subsystem, category: "agent")
}
