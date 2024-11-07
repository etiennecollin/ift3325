pub fn calculate_crc(data: &[u8]) -> u16 {
    let polynomial: u16 = 0x1021;
    let mut remainder: u16 = 0;
    // TODO: Calculer le CRC
    remainder
}

// Convert this typst code to Rust

/*
#let crc-shift-register(inputs, generator) = {
  generator = generator.dedup().sorted()
  let generator_degree = generator.last()

  ////////////////////////////////////////////////////////////////
  // Create table header
  ////////////////////////////////////////////////////////////////
  // Column "n" for steps
  let header = ($n$,)

  // Add register columns
  for i in range(generator_degree).rev() {
    header.push($C_#i$)
  }

  // Add XOR equations
  for gen in generator.slice(0, -1) {
    header.push($C_#(generator.last()-1) xor C_#(gen - 1) xor I$)
  }

  // Last XOR is only affected by input and generator.last()
  header.push($C_#(generator.last()-1) xor I$)

  // Add input column
  header.push($I$)

  ////////////////////////////////////////////////////////////////
  // Fill rows and tails
  ////////////////////////////////////////////////////////////////
  let rows = ()
  let tails = ()
  for i in range(inputs.len() + 1) {
    // Initialize current row
    let current = (0,) * generator_degree

    // If not the first row
    if i != 0 {
      // Propagate values towards next register
      for j in range(generator_degree) {
        current.at(j) = rows.last().at(calc.rem((j - 1), generator_degree))
      }
      // Use previous XOR results to set registers
      for (j, gen) in generator.enumerate() {
        current.at(calc.rem(gen, generator_degree)) = tails.last().at(j)
      }
    }

    // If we're not at the result step, compute XORs
    if i < inputs.len() {
      let input = inputs.at(i)
      let xors = ()
      // Update XOR values
      for gen in generator.slice(0, -1) {
        xors.push(
          current
            .at(generator.last() - 1)
            .bit-xor(current.at(gen - 1))
            .bit-xor(input),
        )
      }
      // Last XOR is only affected by input and generator.last()
      xors.push(current.at(generator.last() - 1).bit-xor(input))

      // Add XORs and input to tail
      let tail = ()
      for xor in xors {
        tail.push(xor)
      }
      tail.push(input)

      // Add tail to tails
      tails.push(tail)
    }

    // Add row to rows
    rows.push(current)
  }

  ////////////////////////////////////////////////////////////////
  // Add step number
  ////////////////////////////////////////////////////////////////
  for i in range(inputs.len() + 1) {
    rows.at(i) = rows.at(i).rev()
    rows.at(i).insert(0, i)
  }

  ////////////////////////////////////////////////////////////////
  // Concatenate rows with tails
  ////////////////////////////////////////////////////////////////
  for (i, row) in rows.zip(tails).enumerate() {
    rows.at(i) = row.flatten()
  }

  ////////////////////////////////////////////////////////////////
  // Setup col sizes
  ////////////////////////////////////////////////////////////////
  let cols = ()
  for i in range(header.len()) {
    if i < (header.len() - generator.len() - 1) {
      cols.push(1fr)
    } else {
      cols.push(auto)
    }
  }

  ////////////////////////////////////////////////////////////////
  // Display table
  ////////////////////////////////////////////////////////////////
  show table: set text(size: 0.8em)
  table(
    columns: cols,
    align: horizon + center,
    ..header,
    ..rows.flatten().map(cell => [$#cell$])
  )
  show table: set text(size: 1em)
}
*/
