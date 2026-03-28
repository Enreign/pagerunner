import Foundation

/// UserDefaults-backed notification preferences.
/// Keys: "notif.daemonHealth", "notif.<profileName>.crash", etc.
struct NotificationSettings {

    static func registerDefaults(profileNames: [String], agentProfiles: Set<String>) {
        var defaults: [String: Any] = [
            "notif.daemonHealth": true,
        ]
        for name in profileNames {
            let isAgent = agentProfiles.contains(name)
            defaults["notif.\(name).crash"] = true          // always on
            defaults["notif.\(name).idle"] = true           // always on
            defaults["notif.\(name).idleMinutes"] = 30
            defaults["notif.\(name).start"] = isAgent       // on for agents, off for personal
        }
        UserDefaults.standard.register(defaults: defaults)
    }

    static func notifyOnDaemonHealth() -> Bool {
        UserDefaults.standard.bool(forKey: "notif.daemonHealth")
    }

    static func notifyOnCrash(profile: String) -> Bool {
        UserDefaults.standard.bool(forKey: "notif.\(profile).crash")
    }

    static func notifyOnIdle(profile: String) -> Bool {
        UserDefaults.standard.bool(forKey: "notif.\(profile).idle")
    }

    static func idleThresholdMinutes(profile: String) -> Int {
        let v = UserDefaults.standard.integer(forKey: "notif.\(profile).idleMinutes")
        return v > 0 ? v : 30
    }

    static func notifyOnStart(profile: String) -> Bool {
        UserDefaults.standard.bool(forKey: "notif.\(profile).start")
    }

    static func setNotifyOnCrash(_ value: Bool, profile: String) {
        UserDefaults.standard.set(value, forKey: "notif.\(profile).crash")
    }

    static func setNotifyOnIdle(_ value: Bool, profile: String) {
        UserDefaults.standard.set(value, forKey: "notif.\(profile).idle")
    }

    static func setIdleThresholdMinutes(_ value: Int, profile: String) {
        UserDefaults.standard.set(value, forKey: "notif.\(profile).idleMinutes")
    }

    static func setNotifyOnStart(_ value: Bool, profile: String) {
        UserDefaults.standard.set(value, forKey: "notif.\(profile).start")
    }

    static func setNotifyOnDaemonHealth(_ value: Bool) {
        UserDefaults.standard.set(value, forKey: "notif.daemonHealth")
    }
}
