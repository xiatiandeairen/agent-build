import Core
import SwiftUI

@main
struct SampleWorkspaceApp: App {
    var body: some Scene {
        WindowGroup { Text(Core.greeting()) }
    }
}
