//! Material Design color palette.
//!
//! Provides all Material Design colors with their various shades.
//! See: <https://m3.material.io/styles/color/the-color-system/color-roles>

use super::color::Color;

/// Material Design color palette.
///
/// This struct provides access to all Material Design colors organized by hue and shade.
/// Each color has variants from 50 (lightest) to 900 (darkest).
///
/// # Examples
///
/// ```
/// use flui_types::MaterialColors;
///
/// let primary = MaterialColors::BLUE_500;
/// let accent = MaterialColors::PINK_A200;
/// ```
pub struct MaterialColors;

impl MaterialColors {
    // ===== Red =====
    /// Red 50
    pub const RED_50: Color = Color::rgb(255, 235, 238);
    /// Red 100
    pub const RED_100: Color = Color::rgb(255, 205, 210);
    /// Red 200
    pub const RED_200: Color = Color::rgb(239, 154, 154);
    /// Red 300
    pub const RED_300: Color = Color::rgb(229, 115, 115);
    /// Red 400
    pub const RED_400: Color = Color::rgb(239, 83, 80);
    /// Red 500
    pub const RED_500: Color = Color::rgb(244, 67, 54);
    /// Red 600
    pub const RED_600: Color = Color::rgb(229, 57, 53);
    /// Red 700
    pub const RED_700: Color = Color::rgb(211, 47, 47);
    /// Red 800
    pub const RED_800: Color = Color::rgb(198, 40, 40);
    /// Red 900
    pub const RED_900: Color = Color::rgb(183, 28, 28);
    /// Red Accent 100
    pub const RED_A100: Color = Color::rgb(255, 138, 128);
    /// Red Accent 200
    pub const RED_A200: Color = Color::rgb(255, 82, 82);
    /// Red Accent 400
    pub const RED_A400: Color = Color::rgb(255, 23, 68);
    /// Red Accent 700
    pub const RED_A700: Color = Color::rgb(213, 0, 0);

    // ===== Pink =====
    /// Pink 50
    pub const PINK_50: Color = Color::rgb(252, 228, 236);
    /// Pink 100
    pub const PINK_100: Color = Color::rgb(248, 187, 208);
    /// Pink 200
    pub const PINK_200: Color = Color::rgb(244, 143, 177);
    /// Pink 300
    pub const PINK_300: Color = Color::rgb(240, 98, 146);
    /// Pink 400
    pub const PINK_400: Color = Color::rgb(236, 64, 122);
    /// Pink 500
    pub const PINK_500: Color = Color::rgb(233, 30, 99);
    /// Pink 600
    pub const PINK_600: Color = Color::rgb(216, 27, 96);
    /// Pink 700
    pub const PINK_700: Color = Color::rgb(194, 24, 91);
    /// Pink 800
    pub const PINK_800: Color = Color::rgb(173, 20, 87);
    /// Pink 900
    pub const PINK_900: Color = Color::rgb(136, 14, 79);
    /// Pink Accent 100
    pub const PINK_A100: Color = Color::rgb(255, 128, 171);
    /// Pink Accent 200
    pub const PINK_A200: Color = Color::rgb(255, 64, 129);
    /// Pink Accent 400
    pub const PINK_A400: Color = Color::rgb(245, 0, 87);
    /// Pink Accent 700
    pub const PINK_A700: Color = Color::rgb(197, 17, 98);

    // ===== Purple =====
    /// Purple 50
    pub const PURPLE_50: Color = Color::rgb(243, 229, 245);
    /// Purple 100
    pub const PURPLE_100: Color = Color::rgb(225, 190, 231);
    /// Purple 200
    pub const PURPLE_200: Color = Color::rgb(206, 147, 216);
    /// Purple 300
    pub const PURPLE_300: Color = Color::rgb(186, 104, 200);
    /// Purple 400
    pub const PURPLE_400: Color = Color::rgb(171, 71, 188);
    /// Purple 500
    pub const PURPLE_500: Color = Color::rgb(156, 39, 176);
    /// Purple 600
    pub const PURPLE_600: Color = Color::rgb(142, 36, 170);
    /// Purple 700
    pub const PURPLE_700: Color = Color::rgb(123, 31, 162);
    /// Purple 800
    pub const PURPLE_800: Color = Color::rgb(106, 27, 154);
    /// Purple 900
    pub const PURPLE_900: Color = Color::rgb(74, 20, 140);
    /// Purple Accent 100
    pub const PURPLE_A100: Color = Color::rgb(234, 128, 252);
    /// Purple Accent 200
    pub const PURPLE_A200: Color = Color::rgb(224, 64, 251);
    /// Purple Accent 400
    pub const PURPLE_A400: Color = Color::rgb(213, 0, 249);
    /// Purple Accent 700
    pub const PURPLE_A700: Color = Color::rgb(170, 0, 255);

    // ===== Deep Purple =====
    /// Deep Purple 50
    pub const DEEP_PURPLE_50: Color = Color::rgb(237, 231, 246);
    /// Deep Purple 100
    pub const DEEP_PURPLE_100: Color = Color::rgb(209, 196, 233);
    /// Deep Purple 200
    pub const DEEP_PURPLE_200: Color = Color::rgb(179, 157, 219);
    /// Deep Purple 300
    pub const DEEP_PURPLE_300: Color = Color::rgb(149, 117, 205);
    /// Deep Purple 400
    pub const DEEP_PURPLE_400: Color = Color::rgb(126, 87, 194);
    /// Deep Purple 500
    pub const DEEP_PURPLE_500: Color = Color::rgb(103, 58, 183);
    /// Deep Purple 600
    pub const DEEP_PURPLE_600: Color = Color::rgb(94, 53, 177);
    /// Deep Purple 700
    pub const DEEP_PURPLE_700: Color = Color::rgb(81, 45, 168);
    /// Deep Purple 800
    pub const DEEP_PURPLE_800: Color = Color::rgb(69, 39, 160);
    /// Deep Purple 900
    pub const DEEP_PURPLE_900: Color = Color::rgb(49, 27, 146);
    /// Deep Purple Accent 100
    pub const DEEP_PURPLE_A100: Color = Color::rgb(179, 136, 255);
    /// Deep Purple Accent 200
    pub const DEEP_PURPLE_A200: Color = Color::rgb(124, 77, 255);
    /// Deep Purple Accent 400
    pub const DEEP_PURPLE_A400: Color = Color::rgb(101, 31, 255);
    /// Deep Purple Accent 700
    pub const DEEP_PURPLE_A700: Color = Color::rgb(98, 0, 234);

    // ===== Indigo =====
    /// Indigo 50
    pub const INDIGO_50: Color = Color::rgb(232, 234, 246);
    /// Indigo 100
    pub const INDIGO_100: Color = Color::rgb(197, 202, 233);
    /// Indigo 200
    pub const INDIGO_200: Color = Color::rgb(159, 168, 218);
    /// Indigo 300
    pub const INDIGO_300: Color = Color::rgb(121, 134, 203);
    /// Indigo 400
    pub const INDIGO_400: Color = Color::rgb(92, 107, 192);
    /// Indigo 500
    pub const INDIGO_500: Color = Color::rgb(63, 81, 181);
    /// Indigo 600
    pub const INDIGO_600: Color = Color::rgb(57, 73, 171);
    /// Indigo 700
    pub const INDIGO_700: Color = Color::rgb(48, 63, 159);
    /// Indigo 800
    pub const INDIGO_800: Color = Color::rgb(40, 53, 147);
    /// Indigo 900
    pub const INDIGO_900: Color = Color::rgb(26, 35, 126);
    /// Indigo Accent 100
    pub const INDIGO_A100: Color = Color::rgb(140, 158, 255);
    /// Indigo Accent 200
    pub const INDIGO_A200: Color = Color::rgb(83, 109, 254);
    /// Indigo Accent 400
    pub const INDIGO_A400: Color = Color::rgb(61, 90, 254);
    /// Indigo Accent 700
    pub const INDIGO_A700: Color = Color::rgb(48, 79, 254);

    // ===== Blue =====
    /// Blue 50
    pub const BLUE_50: Color = Color::rgb(227, 242, 253);
    /// Blue 100
    pub const BLUE_100: Color = Color::rgb(187, 222, 251);
    /// Blue 200
    pub const BLUE_200: Color = Color::rgb(144, 202, 249);
    /// Blue 300
    pub const BLUE_300: Color = Color::rgb(100, 181, 246);
    /// Blue 400
    pub const BLUE_400: Color = Color::rgb(66, 165, 245);
    /// Blue 500
    pub const BLUE_500: Color = Color::rgb(33, 150, 243);
    /// Blue 600
    pub const BLUE_600: Color = Color::rgb(30, 136, 229);
    /// Blue 700
    pub const BLUE_700: Color = Color::rgb(25, 118, 210);
    /// Blue 800
    pub const BLUE_800: Color = Color::rgb(21, 101, 192);
    /// Blue 900
    pub const BLUE_900: Color = Color::rgb(13, 71, 161);
    /// Blue Accent 100
    pub const BLUE_A100: Color = Color::rgb(130, 177, 255);
    /// Blue Accent 200
    pub const BLUE_A200: Color = Color::rgb(68, 138, 255);
    /// Blue Accent 400
    pub const BLUE_A400: Color = Color::rgb(41, 121, 255);
    /// Blue Accent 700
    pub const BLUE_A700: Color = Color::rgb(41, 98, 255);

    // ===== Light Blue =====
    /// Light Blue 50
    pub const LIGHT_BLUE_50: Color = Color::rgb(225, 245, 254);
    /// Light Blue 100
    pub const LIGHT_BLUE_100: Color = Color::rgb(179, 229, 252);
    /// Light Blue 200
    pub const LIGHT_BLUE_200: Color = Color::rgb(129, 212, 250);
    /// Light Blue 300
    pub const LIGHT_BLUE_300: Color = Color::rgb(79, 195, 247);
    /// Light Blue 400
    pub const LIGHT_BLUE_400: Color = Color::rgb(41, 182, 246);
    /// Light Blue 500
    pub const LIGHT_BLUE_500: Color = Color::rgb(3, 169, 244);
    /// Light Blue 600
    pub const LIGHT_BLUE_600: Color = Color::rgb(3, 155, 229);
    /// Light Blue 700
    pub const LIGHT_BLUE_700: Color = Color::rgb(2, 136, 209);
    /// Light Blue 800
    pub const LIGHT_BLUE_800: Color = Color::rgb(2, 119, 189);
    /// Light Blue 900
    pub const LIGHT_BLUE_900: Color = Color::rgb(1, 87, 155);
    /// Light Blue Accent 100
    pub const LIGHT_BLUE_A100: Color = Color::rgb(128, 216, 255);
    /// Light Blue Accent 200
    pub const LIGHT_BLUE_A200: Color = Color::rgb(64, 196, 255);
    /// Light Blue Accent 400
    pub const LIGHT_BLUE_A400: Color = Color::rgb(0, 176, 255);
    /// Light Blue Accent 700
    pub const LIGHT_BLUE_A700: Color = Color::rgb(0, 145, 234);

    // ===== Cyan =====
    /// Cyan 50
    pub const CYAN_50: Color = Color::rgb(224, 247, 250);
    /// Cyan 100
    pub const CYAN_100: Color = Color::rgb(178, 235, 242);
    /// Cyan 200
    pub const CYAN_200: Color = Color::rgb(128, 222, 234);
    /// Cyan 300
    pub const CYAN_300: Color = Color::rgb(77, 208, 225);
    /// Cyan 400
    pub const CYAN_400: Color = Color::rgb(38, 198, 218);
    /// Cyan 500
    pub const CYAN_500: Color = Color::rgb(0, 188, 212);
    /// Cyan 600
    pub const CYAN_600: Color = Color::rgb(0, 172, 193);
    /// Cyan 700
    pub const CYAN_700: Color = Color::rgb(0, 151, 167);
    /// Cyan 800
    pub const CYAN_800: Color = Color::rgb(0, 131, 143);
    /// Cyan 900
    pub const CYAN_900: Color = Color::rgb(0, 96, 100);
    /// Cyan Accent 100
    pub const CYAN_A100: Color = Color::rgb(132, 255, 255);
    /// Cyan Accent 200
    pub const CYAN_A200: Color = Color::rgb(24, 255, 255);
    /// Cyan Accent 400
    pub const CYAN_A400: Color = Color::rgb(0, 229, 255);
    /// Cyan Accent 700
    pub const CYAN_A700: Color = Color::rgb(0, 184, 212);

    // ===== Teal =====
    /// Teal 50
    pub const TEAL_50: Color = Color::rgb(224, 242, 241);
    /// Teal 100
    pub const TEAL_100: Color = Color::rgb(178, 223, 219);
    /// Teal 200
    pub const TEAL_200: Color = Color::rgb(128, 203, 196);
    /// Teal 300
    pub const TEAL_300: Color = Color::rgb(77, 182, 172);
    /// Teal 400
    pub const TEAL_400: Color = Color::rgb(38, 166, 154);
    /// Teal 500
    pub const TEAL_500: Color = Color::rgb(0, 150, 136);
    /// Teal 600
    pub const TEAL_600: Color = Color::rgb(0, 137, 123);
    /// Teal 700
    pub const TEAL_700: Color = Color::rgb(0, 121, 107);
    /// Teal 800
    pub const TEAL_800: Color = Color::rgb(0, 105, 92);
    /// Teal 900
    pub const TEAL_900: Color = Color::rgb(0, 77, 64);
    /// Teal Accent 100
    pub const TEAL_A100: Color = Color::rgb(167, 255, 235);
    /// Teal Accent 200
    pub const TEAL_A200: Color = Color::rgb(100, 255, 218);
    /// Teal Accent 400
    pub const TEAL_A400: Color = Color::rgb(29, 233, 182);
    /// Teal Accent 700
    pub const TEAL_A700: Color = Color::rgb(0, 191, 165);

    // ===== Green =====
    /// Green 50
    pub const GREEN_50: Color = Color::rgb(232, 245, 233);
    /// Green 100
    pub const GREEN_100: Color = Color::rgb(200, 230, 201);
    /// Green 200
    pub const GREEN_200: Color = Color::rgb(165, 214, 167);
    /// Green 300
    pub const GREEN_300: Color = Color::rgb(129, 199, 132);
    /// Green 400
    pub const GREEN_400: Color = Color::rgb(102, 187, 106);
    /// Green 500
    pub const GREEN_500: Color = Color::rgb(76, 175, 80);
    /// Green 600
    pub const GREEN_600: Color = Color::rgb(67, 160, 71);
    /// Green 700
    pub const GREEN_700: Color = Color::rgb(56, 142, 60);
    /// Green 800
    pub const GREEN_800: Color = Color::rgb(46, 125, 50);
    /// Green 900
    pub const GREEN_900: Color = Color::rgb(27, 94, 32);
    /// Green Accent 100
    pub const GREEN_A100: Color = Color::rgb(185, 246, 202);
    /// Green Accent 200
    pub const GREEN_A200: Color = Color::rgb(105, 240, 174);
    /// Green Accent 400
    pub const GREEN_A400: Color = Color::rgb(0, 230, 118);
    /// Green Accent 700
    pub const GREEN_A700: Color = Color::rgb(0, 200, 83);

    // ===== Light Green =====
    /// Light Green 50
    pub const LIGHT_GREEN_50: Color = Color::rgb(241, 248, 233);
    /// Light Green 100
    pub const LIGHT_GREEN_100: Color = Color::rgb(220, 237, 200);
    /// Light Green 200
    pub const LIGHT_GREEN_200: Color = Color::rgb(197, 225, 165);
    /// Light Green 300
    pub const LIGHT_GREEN_300: Color = Color::rgb(174, 213, 129);
    /// Light Green 400
    pub const LIGHT_GREEN_400: Color = Color::rgb(156, 204, 101);
    /// Light Green 500
    pub const LIGHT_GREEN_500: Color = Color::rgb(139, 195, 74);
    /// Light Green 600
    pub const LIGHT_GREEN_600: Color = Color::rgb(124, 179, 66);
    /// Light Green 700
    pub const LIGHT_GREEN_700: Color = Color::rgb(104, 159, 56);
    /// Light Green 800
    pub const LIGHT_GREEN_800: Color = Color::rgb(85, 139, 47);
    /// Light Green 900
    pub const LIGHT_GREEN_900: Color = Color::rgb(51, 105, 30);
    /// Light Green Accent 100
    pub const LIGHT_GREEN_A100: Color = Color::rgb(204, 255, 144);
    /// Light Green Accent 200
    pub const LIGHT_GREEN_A200: Color = Color::rgb(178, 255, 89);
    /// Light Green Accent 400
    pub const LIGHT_GREEN_A400: Color = Color::rgb(118, 255, 3);
    /// Light Green Accent 700
    pub const LIGHT_GREEN_A700: Color = Color::rgb(100, 221, 23);

    // ===== Lime =====
    /// Lime 50
    pub const LIME_50: Color = Color::rgb(249, 251, 231);
    /// Lime 100
    pub const LIME_100: Color = Color::rgb(240, 244, 195);
    /// Lime 200
    pub const LIME_200: Color = Color::rgb(230, 238, 156);
    /// Lime 300
    pub const LIME_300: Color = Color::rgb(220, 231, 117);
    /// Lime 400
    pub const LIME_400: Color = Color::rgb(212, 225, 87);
    /// Lime 500
    pub const LIME_500: Color = Color::rgb(205, 220, 57);
    /// Lime 600
    pub const LIME_600: Color = Color::rgb(192, 202, 51);
    /// Lime 700
    pub const LIME_700: Color = Color::rgb(175, 180, 43);
    /// Lime 800
    pub const LIME_800: Color = Color::rgb(158, 157, 36);
    /// Lime 900
    pub const LIME_900: Color = Color::rgb(130, 119, 23);
    /// Lime Accent 100
    pub const LIME_A100: Color = Color::rgb(244, 255, 129);
    /// Lime Accent 200
    pub const LIME_A200: Color = Color::rgb(238, 255, 65);
    /// Lime Accent 400
    pub const LIME_A400: Color = Color::rgb(198, 255, 0);
    /// Lime Accent 700
    pub const LIME_A700: Color = Color::rgb(174, 234, 0);

    // ===== Yellow =====
    /// Yellow 50
    pub const YELLOW_50: Color = Color::rgb(255, 253, 231);
    /// Yellow 100
    pub const YELLOW_100: Color = Color::rgb(255, 249, 196);
    /// Yellow 200
    pub const YELLOW_200: Color = Color::rgb(255, 245, 157);
    /// Yellow 300
    pub const YELLOW_300: Color = Color::rgb(255, 241, 118);
    /// Yellow 400
    pub const YELLOW_400: Color = Color::rgb(255, 238, 88);
    /// Yellow 500
    pub const YELLOW_500: Color = Color::rgb(255, 235, 59);
    /// Yellow 600
    pub const YELLOW_600: Color = Color::rgb(253, 216, 53);
    /// Yellow 700
    pub const YELLOW_700: Color = Color::rgb(251, 192, 45);
    /// Yellow 800
    pub const YELLOW_800: Color = Color::rgb(249, 168, 37);
    /// Yellow 900
    pub const YELLOW_900: Color = Color::rgb(245, 127, 23);
    /// Yellow Accent 100
    pub const YELLOW_A100: Color = Color::rgb(255, 255, 141);
    /// Yellow Accent 200
    pub const YELLOW_A200: Color = Color::rgb(255, 255, 0);
    /// Yellow Accent 400
    pub const YELLOW_A400: Color = Color::rgb(255, 234, 0);
    /// Yellow Accent 700
    pub const YELLOW_A700: Color = Color::rgb(255, 214, 0);

    // ===== Amber =====
    /// Amber 50
    pub const AMBER_50: Color = Color::rgb(255, 248, 225);
    /// Amber 100
    pub const AMBER_100: Color = Color::rgb(255, 236, 179);
    /// Amber 200
    pub const AMBER_200: Color = Color::rgb(255, 224, 130);
    /// Amber 300
    pub const AMBER_300: Color = Color::rgb(255, 213, 79);
    /// Amber 400
    pub const AMBER_400: Color = Color::rgb(255, 202, 40);
    /// Amber 500
    pub const AMBER_500: Color = Color::rgb(255, 193, 7);
    /// Amber 600
    pub const AMBER_600: Color = Color::rgb(255, 179, 0);
    /// Amber 700
    pub const AMBER_700: Color = Color::rgb(255, 160, 0);
    /// Amber 800
    pub const AMBER_800: Color = Color::rgb(255, 143, 0);
    /// Amber 900
    pub const AMBER_900: Color = Color::rgb(255, 111, 0);
    /// Amber Accent 100
    pub const AMBER_A100: Color = Color::rgb(255, 229, 127);
    /// Amber Accent 200
    pub const AMBER_A200: Color = Color::rgb(255, 215, 64);
    /// Amber Accent 400
    pub const AMBER_A400: Color = Color::rgb(255, 196, 0);
    /// Amber Accent 700
    pub const AMBER_A700: Color = Color::rgb(255, 171, 0);

    // ===== Orange =====
    /// Orange 50
    pub const ORANGE_50: Color = Color::rgb(255, 243, 224);
    /// Orange 100
    pub const ORANGE_100: Color = Color::rgb(255, 224, 178);
    /// Orange 200
    pub const ORANGE_200: Color = Color::rgb(255, 204, 128);
    /// Orange 300
    pub const ORANGE_300: Color = Color::rgb(255, 183, 77);
    /// Orange 400
    pub const ORANGE_400: Color = Color::rgb(255, 167, 38);
    /// Orange 500
    pub const ORANGE_500: Color = Color::rgb(255, 152, 0);
    /// Orange 600
    pub const ORANGE_600: Color = Color::rgb(251, 140, 0);
    /// Orange 700
    pub const ORANGE_700: Color = Color::rgb(245, 124, 0);
    /// Orange 800
    pub const ORANGE_800: Color = Color::rgb(239, 108, 0);
    /// Orange 900
    pub const ORANGE_900: Color = Color::rgb(230, 81, 0);
    /// Orange Accent 100
    pub const ORANGE_A100: Color = Color::rgb(255, 209, 128);
    /// Orange Accent 200
    pub const ORANGE_A200: Color = Color::rgb(255, 171, 64);
    /// Orange Accent 400
    pub const ORANGE_A400: Color = Color::rgb(255, 145, 0);
    /// Orange Accent 700
    pub const ORANGE_A700: Color = Color::rgb(255, 109, 0);

    // ===== Deep Orange =====
    /// Deep Orange 50
    pub const DEEP_ORANGE_50: Color = Color::rgb(251, 233, 231);
    /// Deep Orange 100
    pub const DEEP_ORANGE_100: Color = Color::rgb(255, 204, 188);
    /// Deep Orange 200
    pub const DEEP_ORANGE_200: Color = Color::rgb(255, 171, 145);
    /// Deep Orange 300
    pub const DEEP_ORANGE_300: Color = Color::rgb(255, 138, 101);
    /// Deep Orange 400
    pub const DEEP_ORANGE_400: Color = Color::rgb(255, 112, 67);
    /// Deep Orange 500
    pub const DEEP_ORANGE_500: Color = Color::rgb(255, 87, 34);
    /// Deep Orange 600
    pub const DEEP_ORANGE_600: Color = Color::rgb(244, 81, 30);
    /// Deep Orange 700
    pub const DEEP_ORANGE_700: Color = Color::rgb(230, 74, 25);
    /// Deep Orange 800
    pub const DEEP_ORANGE_800: Color = Color::rgb(216, 67, 21);
    /// Deep Orange 900
    pub const DEEP_ORANGE_900: Color = Color::rgb(191, 54, 12);
    /// Deep Orange Accent 100
    pub const DEEP_ORANGE_A100: Color = Color::rgb(255, 158, 128);
    /// Deep Orange Accent 200
    pub const DEEP_ORANGE_A200: Color = Color::rgb(255, 110, 64);
    /// Deep Orange Accent 400
    pub const DEEP_ORANGE_A400: Color = Color::rgb(255, 61, 0);
    /// Deep Orange Accent 700
    pub const DEEP_ORANGE_A700: Color = Color::rgb(221, 44, 0);

    // ===== Brown =====
    /// Brown 50
    pub const BROWN_50: Color = Color::rgb(239, 235, 233);
    /// Brown 100
    pub const BROWN_100: Color = Color::rgb(215, 204, 200);
    /// Brown 200
    pub const BROWN_200: Color = Color::rgb(188, 170, 164);
    /// Brown 300
    pub const BROWN_300: Color = Color::rgb(161, 136, 127);
    /// Brown 400
    pub const BROWN_400: Color = Color::rgb(141, 110, 99);
    /// Brown 500
    pub const BROWN_500: Color = Color::rgb(121, 85, 72);
    /// Brown 600
    pub const BROWN_600: Color = Color::rgb(109, 76, 65);
    /// Brown 700
    pub const BROWN_700: Color = Color::rgb(93, 64, 55);
    /// Brown 800
    pub const BROWN_800: Color = Color::rgb(78, 52, 46);
    /// Brown 900
    pub const BROWN_900: Color = Color::rgb(62, 39, 35);

    // ===== Grey =====
    /// Grey 50
    pub const GREY_50: Color = Color::rgb(250, 250, 250);
    /// Grey 100
    pub const GREY_100: Color = Color::rgb(245, 245, 245);
    /// Grey 200
    pub const GREY_200: Color = Color::rgb(238, 238, 238);
    /// Grey 300
    pub const GREY_300: Color = Color::rgb(224, 224, 224);
    /// Grey 400
    pub const GREY_400: Color = Color::rgb(189, 189, 189);
    /// Grey 500
    pub const GREY_500: Color = Color::rgb(158, 158, 158);
    /// Grey 600
    pub const GREY_600: Color = Color::rgb(117, 117, 117);
    /// Grey 700
    pub const GREY_700: Color = Color::rgb(97, 97, 97);
    /// Grey 800
    pub const GREY_800: Color = Color::rgb(66, 66, 66);
    /// Grey 900
    pub const GREY_900: Color = Color::rgb(33, 33, 33);

    // ===== Blue Grey =====
    /// Blue Grey 50
    pub const BLUE_GREY_50: Color = Color::rgb(236, 239, 241);
    /// Blue Grey 100
    pub const BLUE_GREY_100: Color = Color::rgb(207, 216, 220);
    /// Blue Grey 200
    pub const BLUE_GREY_200: Color = Color::rgb(176, 190, 197);
    /// Blue Grey 300
    pub const BLUE_GREY_300: Color = Color::rgb(144, 164, 174);
    /// Blue Grey 400
    pub const BLUE_GREY_400: Color = Color::rgb(120, 144, 156);
    /// Blue Grey 500
    pub const BLUE_GREY_500: Color = Color::rgb(96, 125, 139);
    /// Blue Grey 600
    pub const BLUE_GREY_600: Color = Color::rgb(84, 110, 122);
    /// Blue Grey 700
    pub const BLUE_GREY_700: Color = Color::rgb(69, 90, 100);
    /// Blue Grey 800
    pub const BLUE_GREY_800: Color = Color::rgb(55, 71, 79);
    /// Blue Grey 900
    pub const BLUE_GREY_900: Color = Color::rgb(38, 50, 56);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_material_colors_exist() {
        // Test a few colors to ensure they're defined correctly
        assert_eq!(MaterialColors::RED_500, Color::rgb(244, 67, 54));
        assert_eq!(MaterialColors::BLUE_500, Color::rgb(33, 150, 243));
        assert_eq!(MaterialColors::GREEN_500, Color::rgb(76, 175, 80));
        assert_eq!(MaterialColors::PURPLE_500, Color::rgb(156, 39, 176));
    }

    #[test]
    fn test_material_colors_all_opaque() {
        // All Material colors should be fully opaque
        assert!(MaterialColors::RED_500.is_opaque());
        assert!(MaterialColors::BLUE_A200.is_opaque());
        assert!(MaterialColors::GREY_900.is_opaque());
    }

    #[test]
    fn test_material_shades_lighten() {
        // Lower numbers should be lighter (higher RGB values)
        let red_50 = MaterialColors::RED_50;
        let red_900 = MaterialColors::RED_900;

        // 50 should be lighter than 900
        assert!(red_50.r > red_900.r);
    }
}
