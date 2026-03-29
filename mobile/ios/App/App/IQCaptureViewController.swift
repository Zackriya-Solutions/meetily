import UIKit
import Capacitor

class IQCaptureViewController: CAPBridgeViewController {

    override func viewDidLoad() {
        super.viewDidLoad()

        // Set status bar background colour to match the app header
        let statusBarView = UIView()
        statusBarView.backgroundColor = UIColor(red: 0.134, green: 0.427, blue: 0.6, alpha: 1.0) // #2276aa
        statusBarView.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(statusBarView)

        NSLayoutConstraint.activate([
            statusBarView.topAnchor.constraint(equalTo: view.topAnchor),
            statusBarView.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            statusBarView.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            statusBarView.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
        ])

        // Push the webview below the status bar
        webView?.scrollView.contentInsetAdjustmentBehavior = .always
    }

    override var preferredStatusBarStyle: UIStatusBarStyle {
        return .lightContent // White text on blue background
    }
}
