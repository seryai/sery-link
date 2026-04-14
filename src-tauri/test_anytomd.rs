use std::fs;

fn main() {
    let test_file = "/Users/hepang/Documents/resume/miner/矿工智能挖矿软件项目实施计划表.xls";
    let resume_file = "/Users/hepang/Documents/me/miner/矿工智能挖矿软件项目实施计划表.xls";
    
    // Try to convert
    if let Ok(bytes) = fs::read(resume_file) {
        println!("Read {} bytes from resume file", bytes.len());
        
        match anytomd::convert_bytes(&bytes, "docx", &anytomd::ConversionOptions::default()) {
            Ok(result) => {
                println!("✓ Conversion successful!");
                println!("Markdown length: {} chars", result.markdown.len());
                println!("First 500 chars:\n{}", &result.markdown[..500.min(result.markdown.len())]);
            }
            Err(e) => {
                println!("✗ Conversion failed: {}", e);
            }
        }
    } else {
        println!("Could not read file");
    }
}
