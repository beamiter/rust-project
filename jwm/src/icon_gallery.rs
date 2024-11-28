pub const ICON_GALLERY: [&str; 205] = [
    "⛄", "🏂", "⛷ ", "🎿", "🍑", "🍒", "🍓", "🍔", "🍕", "🍖", "🍗", "🍘", "🍟", "🍠", "🍩", "🍪",
    "🍫", "🍬", "🍭", "🍯", "🍰", "🍱", "🍳", "🍵", "👑", "👒", "👔", "👕", "👖", "⚽", "⚾", "🚲",
    "🎾", "🏀", "🏁", "🏆", "🏈", "🏊", "🎠", "🎡", "🎢", "🎣", "🎤", "🎥", "🎦", "🎧", "🎨", "🎩",
    "🎪", "🎫", "🎬", "🎭", "🎮", "🚥", "🚧", "🚨", "🚩", "🚪", "🚹", "🚺", "🚻", "🚼", "🛀", "🌹",
    "🌷", "🌸", "🌺", "🌻", "🌼", "🍀", "☘", "🚀", "🚃", "🚄", "🚅", "🚇", "🚌", "🚑", "🚒", "🚓",
    "🚕", "🚗", "🚙", "🚚", "🚢", "🚤", "🍑", "🍒", "🍓", "🍔", "🍕", "🍖", "🍗", "🍘", "🍤", "🍥",
    "🍦", "🍩", "🍫", "🍬", "🍭", "🍰", "✳", "✴", "🔯", "🌠", "❇", "🌔", "🍃", "🌏", "☎", "📳",
    "📴", "💝", "👑", "🔥", "☕", "♨", "🤒", "☣", "☃", "🤤", "🦃", "🍁", "🍂", "🌽", "🥓", "🥂",
    "🏡", "🏈", "⛵", "🌺", "🎼", "🎤", "🎷", "🎸", "🎹", "🎺", "🎻", "🥁", "🎵", "🎶", "🎛", "🎙",
    "🐿", "🐮", "🐭", "🐫", "🐯", "🐰", "🐵", "🐹", "🦊", "🐶", "🦁", "🐻", "❄️ ", "🐔", "🐥", "🐧",
    "🦜", "🦚", "🦅", "🦆", "🐦", "🔥", "🐸", "🐢", "🐊", "🐉", "🐲", "🦖", "🐋", "🐳", "🐟", "🐙",
    "🐬", "🐡", "🐠", "🦭", "🦈", "🦞", "🦀", "🦑", "🦐", "🐝", "🐌", "🦟", "🪰", "🍄", "🌲", "🍁",
    "🪴", "🚕", "🚓", "🚒", "🚑", "🚏", "🚌", "🚉", "🚇", "🚅", "🚄", "🚃", "🚀",
];

use rand::seq::SliceRandom;
pub fn generate_random_tags() -> Vec<&'static str> {
    let mut numbers: Vec<usize> = (0..ICON_GALLERY.len()).collect();

    // Shuffle the vector
    let mut rng = rand::thread_rng();
    numbers.shuffle(&mut rng);

    // Take the first 9 elements to get 9 unique random values
    let random_indices: Vec<_> = numbers.into_iter().take(9).collect();
    // Print the random numbers
    println!("Random indices: {:?}", random_indices);

    // Use the random indices to access elements in 'data'
    let random_elements: Vec<&str> = random_indices
        .iter()
        .map(|&index| ICON_GALLERY[index])
        .collect();

    // Print the random elements
    println!("Random elements: {:?}", random_elements);
    return random_elements;
}
