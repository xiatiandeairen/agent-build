import SwiftUI

@main
struct SampleApp: App {
    var body: some Scene {
        WindowGroup { Text(Greeting.text) }
    }
}

enum Greeting {
    static let text = "Hello from agent-build"
}
