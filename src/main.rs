mod create_meta;

use crate::create_meta::{create_meta_metallic_map, create_meta_normal_map, create_meta_texture};
use image::{ImageBuffer, Rgba};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

fn main() {
    println!("🚀 Запуск ...");

    let current_dir = std::env::current_dir().expect("Не удалось получить текущую папку");
    println!("📁 Рабочая папка: {}", current_dir.display());

    // Создаём папку textures
    let textures_dir = current_dir.join("textures");
    if !textures_dir.exists() {
        fs::create_dir(&textures_dir).expect("Не удалось создать папку textures");
        println!("📁 Создана папка: textures");
    }

    // Поиск файлов текстур
    let mut files = Vec::new();
    for entry in WalkDir::new(&current_dir)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                // только png файлы
                if ext == "png" {
                    if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                        let name_lower = name.to_lowercase();
                        if name_lower.ends_with("basecolor")
                            || name_lower.ends_with("normal")
                            || name_lower.ends_with("metallic")
                            || name_lower.ends_with("roughness")
                        {
                            files.push(path.to_path_buf());
                        }
                    }
                }
            }
        }
    }

    if files.is_empty() {
        println!(
            "⚠️ Не найдено файлов текстур с суффиксами: BaseColor, Normal, Metallic, Roughness"
        );
        return;
    }

    println!("🔍 Найдено {} файлов текстур", files.len());

    // Группировка текстур по базовому имени
    let mut texture_groups: std::collections::HashMap<String, TextureSet> =
        std::collections::HashMap::new();

    for file_path in files {
        let file_stem = file_path.file_stem().unwrap().to_str().unwrap();
        let base_name = clean_base_name(file_stem);
        let suffix = detect_suffix(file_stem);

        let entry = texture_groups
            .entry(base_name)
            .or_insert(TextureSet::default());
        match suffix {
            Suffix::BaseColor => entry.base_color = Some(file_path),
            Suffix::Normal => entry.normal = Some(file_path),
            Suffix::Metallic => entry.metallic = Some(file_path),
            Suffix::Roughness => entry.roughness = Some(file_path),
        }
    }

    // Обработка каждой группы
    for (base_name, texture_set) in texture_groups {
        println!("\n📦 Обработка: {}", base_name);

        // Копируем BaseColor
        if let Some(base_path) = &texture_set.base_color {
            let dest_path = textures_dir.join(format!("{}{}", base_name, "_BaseColor.png"));
            fs::copy(base_path, &dest_path).expect("Ошибка копирования BaseColor");
            println!("  ✅ BaseColor скопирована");

            // Отключаем sRGB через .meta файл
            create_meta_texture(&dest_path, false);
        }

        // Копируем Normal
        if let Some(normal_path) = &texture_set.normal {
            let dest_path = textures_dir.join(format!("{}{}", base_name, "_Normal.png"));
            fs::copy(normal_path, &dest_path).expect("Ошибка копирования Normal");
            println!("  ✅ Normal скопирована");

            // Отключаем sRGB для Normal карты
            create_meta_normal_map(&dest_path);
        }

        // Создаём Metallic + Smoothness в альфа-канале
        if let (Some(metallic_path), Some(roughness_path)) =
            (&texture_set.metallic, &texture_set.roughness)
        {
            match create_metallic_smoothness_texture(
                metallic_path,
                roughness_path,
                &textures_dir,
                &base_name,
            ) {
                Ok(path) => {
                    println!("  ✅ Metallic + Smoothness создана: {}", path.display());
                    // Отключаем sRGB и настраиваем альфа-канал
                    create_meta_metallic_map(&path);
                }
                Err(e) => println!("  ❌ Ошибка создания Metallic+Smoothness: {}", e),
            }
        } else {
            if texture_set.metallic.is_none() {
                println!("  ⚠️ Нет Metallic текстуры для {}", base_name);
            }
            if texture_set.roughness.is_none() {
                println!("  ⚠️ Нет Roughness текстуры для {}", base_name);
            }
        }
    }

    println!("\n✨ Готово! Все текстуры подготовлены в папке 'textures'");
    println!("📌 Не забудь в Unity:");
    println!("   - Для _MetallicSmoothness.png: отключить sRGB (мы уже создали .meta файл)");
    println!("   - В материале: Surface Options → Workflow Mode = Metallic");
    println!("   - В Metallic Map: Smoothness Source = Metallic Alpha");
}

#[derive(Default)]
struct TextureSet {
    base_color: Option<PathBuf>,
    normal: Option<PathBuf>,
    metallic: Option<PathBuf>,
    roughness: Option<PathBuf>,
}

enum Suffix {
    BaseColor,
    Normal,
    Metallic,
    Roughness,
}

fn detect_suffix(filename: &str) -> Suffix {
    let lower = filename.to_lowercase();
    if lower.ends_with("basecolor") {
        Suffix::BaseColor
    } else if lower.ends_with("normal") {
        Suffix::Normal
    } else if lower.ends_with("metallic") {
        Suffix::Metallic
    } else if lower.ends_with("roughness") {
        Suffix::Roughness
    } else {
        panic!("Неизвестный суффикс: {}", filename);
    }
}

fn clean_base_name(filename: &str) -> String {
    // Удаляем _gameasset (в любом регистре) и _суффикс_типа (в любом регистре)
    let re = Regex::new(r"(?i)_(gameasset|basecolor|normal|metalic|roughness)$").unwrap();
    re.replace_all(filename, "").to_string()
}

fn create_metallic_smoothness_texture(
    metallic_path: &Path,
    roughness_path: &Path,
    output_dir: &Path,
    base_name: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    // Загружаем металлик (RGB)
    let metallic_img = image::open(metallic_path)?;
    let metallic_rgb = metallic_img.to_rgba8();

    // Загружаем roughness
    let roughness_img = image::open(roughness_path)?;
    let roughness_gray = roughness_img.to_luma8();

    // Проверяем совпадение размеров
    let (width, height) = metallic_rgb.dimensions();
    let (r_width, r_height) = roughness_gray.dimensions();

    if width != r_width || height != r_height {
        return Err(format!(
            "Размеры текстур не совпадают: Metallic {}x{}, Roughness {}x{}",
            width, height, r_width, r_height
        )
        .into());
    }

    // Создаём новое изображение RGBA
    let mut output = ImageBuffer::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let metallic_pixel = metallic_rgb.get_pixel(x, y);
            let roughness_value = roughness_gray.get_pixel(x, y)[0];

            // Инвертируем roughness (1.0 - roughness) -> smoothness
            let smoothness = 255 - roughness_value;

            // RGB каналы берём из металлика, Alpha - это smoothness
            let new_pixel = Rgba([
                metallic_pixel[0],
                metallic_pixel[1],
                metallic_pixel[2],
                smoothness,
            ]);

            output.put_pixel(x, y, new_pixel);
        }
    }

    let output_path = output_dir.join(format!("{}{}", base_name, "_MetallicSmoothness.png"));
    output.save(&output_path)?;

    Ok(output_path)
}
