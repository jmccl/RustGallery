
# Serving from S3

Hosting galleries on S3 is not required. I'm just putting some documentation here
because it's useful to remind myself what I did and may be helpful to others.

I use [mountpoint-s3](https://github.com/awslabs/mountpoint-s3) to host
the photos, which mounts an S3 bucket as a directory in the local file system.

### AWS Credentials

There are [many ways](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/credentials.html) 
to set up AWS credentials. Any of those should work.

I'll describe what I'm doing but I'm not suggesting this is a best practice.

I created an AWS user for the sole purpose of having read access to the bucket
I'm using. (If you want to make your S3 bucket readable by the public there's
no need for this, but that may not be a great idea.) I then created an _Access Key_
for that user. I then created a _Bucket Policy_ for the S3 bucket as follows.

```
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Sid": "AllowReadAccessToMyBucket",
            "Effect": "Allow",
            "Principal": {
                "AWS": "arn:aws:iam::<iam id>:user/<username>"
            },
            "Action": [
                "s3:GetObject",
                "s3:ListBucket"
            ],
            "Resource": [
                "arn:aws:s3:::<bucket name>/*",
                "arn:aws:s3:::<bucket nameq>"
            ]
        }
    ]
}
```

### Mounting S3

I'm using the environment variable method from the link above to pass the credentials.
The template for the script I use to mount the bucket is [here](mounts3.template), which
mounts the bucket so that only the nginx child process user has any access to the mount.

Sometimes it is helpful to have access to the mounted 
directory, which is awkward because the child process user has no password or
home directory, so normal methods of impersonation don't work. However, this will do it:

```
    sudo -u http <nginx child process user> bash
```

# Getting Galleries to S3

To copy a directory (gallery) to s3:

```
   aws s3 cp <directory> s3://<bucketname>/<directory> --recursive
```
