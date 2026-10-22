use std::sync::LazyLock;
use serde::Deserialize;
use serde_json::{json, Value};

pub fn merge_json(a: &mut Value, b: Value) {
    if let Value::Object(a) = a && let Value::Object(b) = b {
        for (k, v) in b {
            if v.is_null() {
                a.remove(&k);
            }
            else {
                merge_json(a.entry(k).or_insert(Value::Null), v);
            }
        }
        return;
    }

    *a = b;
}

pub fn _inventory_icon(frame_size: u8, mut icon: Value) -> Value {
    if frame_size != 0 {
        icon.as_array_mut().unwrap().insert(0, json!({
            "image" : format!("/assetmissing.png?crop=0;0;1;1?multiply=0000?setcolor=f00?replace;0000=fff?scalenearest={}", frame_size)
        }));
    }
    icon
}

pub static ITEM_TEMPLATE: LazyLock<Value> = LazyLock::new(|| json!({
    "name" : "perfectlygenericitem",
    "count" : 1,
    "parameters" : {
        "description" : "Generated Item",
        "tooltipKind" : "object",
        "rarity" : "Common",
        "leveledStatusEffects" : []
    }
}));

#[derive(Deserialize)]
pub struct FrameEntry {
    #[allow(dead_code)]
    pub frame: String,
    pub image: String,
    pub pos: [u32; 2],
    pub lower_left: [u8; 4],
    pub lower_right: [u8; 4],
    pub upper_left: [u8; 4],
    pub upper_right: [u8; 4]
}

#[derive(Deserialize)]
pub struct FrameSet {
    pub prefix: String,
    pub entries: Vec<FrameEntry>
}

macro_rules! frameset {
    ($name:ident, $file:literal) => {
        pub static $name: LazyLock<FrameSet> = LazyLock::new(|| {
            serde_json::from_str(include_str!(concat!("framesets/", $file, ".json"))).expect(concat!("Deserialization failed: ", $file))
        });
    };
}

frameset!(SINGLE, "single");
frameset!(CHEST, "chest");
frameset!(PANTS, "pants");
frameset!(FROG_PANTS, "frog_pants");
frameset!(BACK, "back");
frameset!(FEMALE, "female");
frameset!(MALE, "male");
frameset!(FEMALE_GLITCH, "female_glitch");
frameset!(MALE_GLITCH, "male_glitch");
frameset!(FEMALE_NOVAKID, "female_novakid");
frameset!(MALE_NOVAKID, "male_novakid");
frameset!(MALE_HYLOTL, "male_hylotl");

pub const SCALE_FIXERS: &[f32; 256] = &[0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5, 8.5, 9.500003, 10.5, 11.500001, 12.500003, 13.499997, 14.500001, 15.5, 16.5, 17.5, 18.50001, 19.5, 20.499994, 21.499989, 22.5, 23.5, 24.50001, 25.5, 26.500017, 27.49998, 28.500008, 29.50001, 30.49998, 31.5, 32.5, 33.500023, 34.500004, 35.499973, 36.49998, 37.50003, 38.499985, 39.499966, 40.50002, 41.500023, 42.5, 43.500015, 44.500008, 45.5, 46.50004, 47.49994, 48.499996, 49.49995, 50.500008, 51.50003, 52.5, 53.49997, 54.49997, 55.500027, 56.499977, 57.499935, 58.5, 59.5, 60.500076, 61.50005, 62.500107, 63.500023, 64.49997, 65.49992, 66.500244, 67.499825, 68.49996, 69.49994, 70.49991, 71.50011, 72.49996, 73.49982, 74.49984, 75.49992, 76.5, 77.50007, 78.500175, 79.50005, 80.499855, 81.50016, 82.50007, 83.49989, 84.50039, 85.50031, 86.50011, 87.499825, 88.50003, 89.500435, 90.4998, 91.49978, 92.49965, 93.500244, 94.50034, 95.49981, 96.50009, 97.5, 98.50004, 99.50014, 100.50028, 101.499855, 102.500084, 103.49981, 104.49969, 105.49983, 106.499695, 107.500084, 108.4998, 109.4997, 110.5, 111.50021, 112.499855, 113.49991, 114.49992, 115.50022, 116.50034, 117.49985, 118.4999, 119.5001, 120.5, 121.50006, 122.49988, 123.49991, 124.49977, 125.5, 126.50019, 127.5, 128.49998, 129.49979, 130.50005, 131.49937, 132.5005, 133.50002, 134.49963, 135.49901, 136.5, 137.50012, 138.50015, 139.5009, 140.49976, 141.49988, 142.49982, 143.50047, 144.49913, 145.50041, 146.5003, 147.49979, 148.49985, 149.50023, 150.50069, 151.49959, 152.50072, 153.49976, 154.50055, 155.50008, 156.4995, 157.5, 158.49991, 159.50044, 160.49991, 161.49951, 162.50063, 163.49991, 164.49864, 165.49985, 166.50008, 167.5008, 168.50006, 169.49936, 170.50014, 171.50056, 172.5004, 173.49937, 174.5009, 175.50122, 176.50009, 177.49911, 178.5, 179.50072, 180.49916, 181.50087, 182.4999, 183.49994, 184.50085, 185.49838, 186.50052, 187.50099, 188.49957, 189.50024, 190.50081, 191.50105, 192.5008, 193.49991, 194.50038, 195.49991, 196.50052, 197.49983, 198.49995, 199.50073, 200.49973, 201.49907, 202.50107, 203.5008, 204.50041, 205.49988, 206.49898, 207.5001, 208.50066, 209.50044, 210.49931, 211.49973, 212.49893, 213.49948, 214.49852, 215.49864, 216.49976, 217.49895, 218.49886, 219.49939, 220.50043, 221.49895, 222.50064, 223.49953, 224.50142, 225.50018, 226.49876, 227.5, 228.50082, 229.49792, 230.50056, 231.49928, 232.5002, 233.5001, 234.4988, 235.49945, 236.49867, 237.4997, 238.49905, 239.50002, 240.49908, 241.49956, 242.50136, 243.50093, 244.50165, 245.49988, 246.49901, 247.49902, 248.4998, 249.5013, 250.49968, 251.49854, 252.50157, 253.50116, 254.50099, 255.50093];
