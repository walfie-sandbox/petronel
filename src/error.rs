error_chain!{
    errors {
        Twitter {
            description("Twitter streaming error")
        }
        Json(s: String) {
            description("could not parse JSON")
            display("failed to parse JSON: {}", s)
        }
        Closed {
            description("channel closed by sender")
        }
    }
}
