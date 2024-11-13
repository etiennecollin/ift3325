type PolynomialSize = u16;
const POLYNOMIAL: PolynomialSize = 0x1021;
const POLYNOMIAL_WIDTH: usize = 16;
const MSB: PolynomialSize = 1 << (POLYNOMIAL_WIDTH - 1);
// const INITIAL_VALUE: PolynomialSize = PolynomialSize::MAX;
const INITIAL_VALUE: PolynomialSize = 0;
const FINAL_XOR: PolynomialSize = 0;
const LUT: [PolynomialSize; 256] = lookup_table();

/// Generate a lookup table for the CRC-16 algorithm.
/// The table is an array of 256 elements, each element representing the
/// remainder of the polynomial for the corresponding byte value.
///
/// The implementation was inspired by:
/// - <https://barrgroup.com/blog/crc-series-part-3-crc-implementation-code-cc>
const fn lookup_table() -> [PolynomialSize; 256] {
    let mut table = [0; 256];
    let mut input: usize = 0;

    // For each possible byte value
    while input < 256 {
        // Place the input byte in the upper byte of the remainder
        let mut remainder = (input as PolynomialSize) << (POLYNOMIAL_WIDTH - 8);
        let mut bit = 0;

        // Iterate over the 8 bits
        while bit < 8 {
            // Check if the most significant bit is set to 1
            remainder = if remainder & MSB != 0 {
                (remainder << 1) ^ POLYNOMIAL
            } else {
                remainder << 1
            };
            bit += 1;
        }

        table[input] = remainder;
        input += 1;
    }
    table
}

/// CRC-16 CCITT implementation.
/// This function computes the CRC-16 CCITT checksum for the given data.
pub fn crc_16_ccitt(data: &[u8]) -> PolynomialSize {
    let mut remainder = INITIAL_VALUE;
    // For each byte in the data, find its corresponding value in the lookup table and XOR it with
    // the remainder. Then, shift the remainder 8 bits to the left.
    data.iter().for_each(|byte| {
        // Isolate the upper byte of the current remainder
        let processed_byte = *byte ^ (remainder >> (POLYNOMIAL_WIDTH - 8)) as u8;
        // XOR the remainder with the value from the lookup table
        remainder = LUT[processed_byte as usize] ^ (remainder << 8);
    });
    remainder ^ FINAL_XOR
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_16_ccitt_string_test() {
        let data = b"Hello World!";
        let crc = crc_16_ccitt(data);
        assert_eq!(crc, 0xCD3);
    }

    #[test]
    fn crc_16_ccitt_array_test() {
        let data: &[u8] = &[0x00, 0x01, 0x02, 0x03, 0x04];
        let crc = crc_16_ccitt(data);
        assert_eq!(crc, 0xD03);
    }
}
