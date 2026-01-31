use glam::*;

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub struct OctreePos {
    pub coords: IVec3,
    pub depth: u32,
}

#[derive(Debug, Clone)]
pub struct Octree {
    pub data: Vec<u32>,
    pub depth: u32,
}

pub const fn get_octree_idx(cords: IVec3, depth: u32) -> i32 {
    let x = (cords.x >> depth) & 1;
    let y = (cords.y >> depth) & 1;
    let z = (cords.z >> depth) & 1;

    x | (y << 1) | (z << 2)
}

impl Octree {
    pub const fn get_oct_inverted(&self, cords: IVec3, i: u32) -> i32 {
        let depth = self.depth - i;
        get_octree_idx(cords, depth)
    }

    pub fn store(&mut self, position: IVec3, val: image::Rgba<u8>) {
        let node = OctreePos {
            coords: position,
            depth: self.depth,
        };

        // bc floating point error some erroneous voxels
        if node.coords.min_element() < 1
            || node.coords.max_element() >= ((1 << (self.depth + 1)) - 1)
        {
            return;
        }

        self.insert(&node, val);
    }
}

pub mod octree_header {
    pub const EXISTS_OFFSET: u32 = 0;
    pub const FINAL_OFFSET: u32 = 8;
    pub const TAG_OFFSET: u32 = 24;

    pub const HEADER_TAG: u8 = 68;

    pub const fn from_color(color: image::Rgba<u8>) -> u32 {
        let [r, g, b, a] = color.0;
        u32::from_le_bytes([r, g, b, a])
    }

    pub const fn to_color(offset: u32) -> image::Rgba<u8> {
        let [r, g, b, a] = offset.to_le_bytes();
        image::Rgba([r, g, b, a])
    }

    pub fn set_header_tag(header: &mut u32) {
        *header |= (HEADER_TAG as u32) << TAG_OFFSET;
    }

    pub const fn get_exists(header: u32, idx: u32) -> bool {
        ((header >> (idx + EXISTS_OFFSET)) & 1) != 0
    }

    pub fn set_exists(header: &mut u32, idx: u32) {
        *header |= 1 << (idx + EXISTS_OFFSET);
    }

    pub const fn get_final(header: u32, idx: u32) -> bool {
        ((header >> (idx + FINAL_OFFSET)) & 1) != 0
    }

    pub fn set_final(header: &mut u32, idx: u32) {
        *header |= 1 << (idx + FINAL_OFFSET)
    }
}

#[derive(Debug, Clone)]
pub struct IterStruct {
    pub offset: u32,
    pub cords: OctreePos,
}

const OCT_PERMS: [IVec3; 8] = {
    let mut cube: [IVec3; 8] = [IVec3::new(0, 0, 0); 8];
    let mut counter: i32 = 0;

    while counter < 8 {
        cube[counter as usize] = IVec3::new(counter & 1, (counter >> 1) & 1, (counter >> 2) & 1);

        counter += 1;
    }

    cube
};

impl Octree {
    pub fn new(depth: u32) -> Self {
        let mut output = Self {
            depth,
            data: Vec::new(),
        };
        output.create_new_oct(0);

        output
    }

    pub fn create_new_oct(&mut self, mut header: u32) -> usize {
        self.data.reserve(9);
        let old_len = self.data.len();
        octree_header::set_header_tag(&mut header);

        unsafe {
            self.data.set_len(old_len + 9);
            self.data[old_len] = header;
            for i in 0..8 {
                self.data[old_len + 1 + i] = 69420420;
            }
        }
        old_len
    }

    pub fn insert(&mut self, node: &OctreePos, value: image::Rgba<u8>) -> Option<u32> {
        if node.depth > self.depth {
            return None;
        }

        let mut current_pointer: u32 = 0;
        let mut current_oct = self.get_oct_inverted(node.coords, 0) as u32;
        let mut current_node = current_pointer + 1 + current_oct as u32;
        let mut inserted = true;

        for d in 0..node.depth {
            let current_header = self.data[current_pointer as usize];
            let next_oct = self.get_oct_inverted(node.coords, d + 1) as u32;

            current_pointer =
                if octree_header::get_exists(current_header, current_oct as u32) && inserted {
                    if octree_header::get_final(current_header, current_oct as u32) {
                        return None;
                    }

                    self.data[current_node as usize]
                } else {
                    let mut next_header = 0;
                    octree_header::set_exists(&mut next_header, next_oct as u32);
                    let next_pointer = self.create_new_oct(next_header) as u32;

                    octree_header::set_exists(
                        &mut self.data[current_pointer as usize],
                        current_oct as u32,
                    );
                    self.data[current_node as usize] = next_pointer;
                    inserted = false;

                    next_pointer
                };

            current_node = current_pointer + 1 + next_oct as u32;
            current_oct = next_oct;
        }

        let next_node = current_pointer + 1 + current_oct as u32;
        let current_header = self.data.get_mut(current_pointer as usize);

        let current_header = current_header.unwrap();

        if octree_header::get_exists(*current_header, current_oct as u32) && inserted {
            return None;
        }

        octree_header::set_exists(current_header, current_oct as u32);
        octree_header::set_final(current_header, current_oct as u32);

        self.data[next_node as usize] = octree_header::from_color(value);

        Some(next_node)
    }

    //replace with non recursive implementation
    fn collect_recursive(&self, nodes: &mut Vec<(OctreePos, u32)>, iter_level: IterStruct) {
        let header = self.data[iter_level.offset as usize];

        for i in 0..8 {
            if !octree_header::get_exists(header, i) {
                continue;
            }

            let scale = 1 << (self.depth - iter_level.cords.depth);
            let coords = OCT_PERMS[i as usize] * scale;
            let new_position = iter_level.cords.coords + coords;
            let offset = self.data[(iter_level.offset + 1 + i) as usize];

            if octree_header::get_final(header, i) {
                let cords = OctreePos {
                    coords: new_position,
                    depth: iter_level.cords.depth,
                };
                nodes.push((cords, offset));
            } else {
                let cords = OctreePos {
                    coords: new_position,
                    depth: iter_level.cords.depth + 1,
                };
                let new_iter = IterStruct { cords, offset };
                self.collect_recursive(nodes, new_iter);
            }
        }
    }

    pub fn collect_nodes(&self) -> Vec<(OctreePos, u32)> {
        let length = self.data.len() / 9;
        let mut collected: Vec<(OctreePos, u32)> = Vec::with_capacity(length);
        let cords = OctreePos {
            coords: IVec3::ZERO,
            depth: 0,
        };
        let first_iter = IterStruct { cords, offset: 0 };

        self.collect_recursive(&mut collected, first_iter);

        collected
    }
}
