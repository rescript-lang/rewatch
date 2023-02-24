echo "Base compilation"
cd ../testrepo

echo "-- Assert testrepo is clean"
git diff --exit-code

echo "-- Clean testrepo"
../target/release/rewatch build . &> /dev/null

echo "-- Build testrepo"
../target/release/rewatch build . &> /dev/null

echo "-- Make sure there are no changes"
git diff --exit-code

echo "-- Make sure output it still correct"

if node ../testrepo/packages/main/src/Main.mjs | grep -z '01\n02\n03' &> /dev/null; 
then
  echo "Output is correct"
else 
  echo "Output is incorrect"
  exit 1
fi
