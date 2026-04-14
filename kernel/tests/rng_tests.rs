//! RNG tests

#[cfg(test)]
mod tests {
    use staros_kernel::crypto::rng::CryptoRng;
    
    #[test]
    fn test_rng_distribution() {
        // TODO: Test that RNG output is uniformly distributed
    }
    
    #[test]
    fn test_rng_no_repeats() {
        // TODO: Test that RNG doesn't repeat
    }
}
