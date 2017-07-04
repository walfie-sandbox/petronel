error_chain!{
    errors {
        Twitter {
            description("Twitter streaming error")
        }
        Json {
            description("could not parse JSON")
        }
    }
}
